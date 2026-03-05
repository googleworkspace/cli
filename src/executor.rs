// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! API Request Execution
//!
//! Handles building and dispatching HTTP requests to Google Workspace APIs.
//! Responsibilities include multipart file uploads, response pagination,
//! error mapping, and optionally running text content through Model Armor for sanitization.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::Context;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};
use tokio::io::AsyncWriteExt;

use crate::discovery::{RestDescription, RestMethod};
use crate::error::GwsError;

/// Tracks what authentication method was used for the request.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthMethod {
    /// OAuth2 bearer token from credentials file
    OAuth,
    /// No authentication was provided
    None,
}

/// Resolve authentication, skipping OAuth when a custom API endpoint is set.
///
/// When `GOOGLE_WORKSPACE_CLI_API_BASE_URL` is set, all requests go to a mock
/// server that doesn't need (or support) Google OAuth. This helper centralises
/// that check so every call-site doesn't need to know about the env var.
///
/// Returns `Err` when OAuth fails so call-sites can log context-appropriate
/// warnings (e.g. `[gws mcp]` prefix in MCP mode).
pub async fn resolve_auth(
    scopes: &[&str],
    account: Option<&str>,
) -> anyhow::Result<(Option<String>, AuthMethod)> {
    if crate::discovery::custom_api_base_url().is_some() {
        return Ok((None, AuthMethod::None));
    }
    match crate::auth::get_token(scopes, account).await {
        Ok(t) => Ok((Some(t), AuthMethod::OAuth)),
        Err(e) => Err(e),
    }
}

/// Configuration for auto-pagination.
#[derive(Debug, Clone)]
pub struct PaginationConfig {
    /// Whether to auto-paginate through all pages.
    pub page_all: bool,
    /// Maximum number of pages to fetch (default: 10).
    pub page_limit: u32,
    /// Delay between page fetches in milliseconds (default: 100).
    pub page_delay_ms: u64,
}

impl Default for PaginationConfig {
    fn default() -> Self {
        Self {
            page_all: false,
            page_limit: 10,
            page_delay_ms: 100,
        }
    }
}

/// Parsed and validated inputs ready for request execution.
#[allow(dead_code)]
struct ExecutionInput {
    params: Map<String, Value>,
    body: Option<Value>,
    full_url: String,
    query_params: HashMap<String, String>,
    is_upload: bool,
}

/// Parse parameters and body JSON, validate against schema, check required params, and build the URL.
fn parse_and_validate_inputs(
    doc: &RestDescription,
    method: &RestMethod,
    params_json: Option<&str>,
    body_json: Option<&str>,
    upload_path: Option<&str>,
) -> Result<ExecutionInput, GwsError> {
    let params: Map<String, Value> = if let Some(p) = params_json {
        serde_json::from_str(p)
            .map_err(|e| GwsError::Validation(format!("Invalid --params JSON: {e}")))?
    } else {
        Map::new()
    };

    let body: Option<Value> = if let Some(b) = body_json {
        let val: Value = serde_json::from_str(b)
            .map_err(|e| GwsError::Validation(format!("Invalid --json body: {e}")))?;

        if let Some(ref req_ref) = method.request {
            if let Some(ref schema_name) = req_ref.schema_ref {
                validate_body_against_schema(&val, schema_name, doc)?;
            }
        }

        Some(val)
    } else {
        None
    };

    for param_name in &method.parameter_order {
        if let Some(param_def) = method.parameters.get(param_name) {
            if param_def.required
                && param_def.location.as_deref() == Some("path")
                && !params.contains_key(param_name)
            {
                return Err(GwsError::Validation(format!(
                    "Required path parameter {} is missing. Provide it via --params",
                    param_name
                )));
            }
        }
    }

    for (param_name, param_def) in &method.parameters {
        if param_def.required && !params.contains_key(param_name) {
            return Err(GwsError::Validation(format!(
                "Required parameter '{}' is missing. Provide it via --params",
                param_name
            )));
        }
    }

    let (full_url, query_params) = build_url(doc, method, &params, upload_path.is_some())?;
    let is_upload = upload_path.is_some() && method.supports_media_upload;

    Ok(ExecutionInput {
        params,
        body,
        full_url,
        query_params,
        is_upload,
    })
}

/// Build an HTTP request with auth, query params, page token, and body/multipart attachment.
#[allow(clippy::too_many_arguments)]
async fn build_http_request(
    client: &reqwest::Client,
    method: &RestMethod,
    input: &ExecutionInput,
    token: Option<&str>,
    auth_method: &AuthMethod,
    page_token: Option<&str>,
    pages_fetched: u32,
    upload_path: Option<&str>,
) -> Result<reqwest::RequestBuilder, GwsError> {
    let mut request = match method.http_method.as_str() {
        "GET" => client.get(&input.full_url),
        "POST" => client.post(&input.full_url),
        "PUT" => client.put(&input.full_url),
        "PATCH" => client.patch(&input.full_url),
        "DELETE" => client.delete(&input.full_url),
        other => {
            return Err(GwsError::Other(anyhow::anyhow!(
                "Unsupported HTTP method: {other}"
            )))
        }
    };

    if let Some(token) = token {
        if *auth_method == AuthMethod::OAuth {
            request = request.bearer_auth(token);
        }
    }

    for (key, value) in &input.query_params {
        request = request.query(&[(key, value)]);
    }

    if let Some(pt) = page_token {
        request = request.query(&[("pageToken", pt)]);
    }

    if pages_fetched == 0 {
        if input.is_upload {
            let upload_path = upload_path.expect("upload_path must be Some when is_upload is true");

            let file_bytes = tokio::fs::read(upload_path).await.map_err(|e| {
                GwsError::Validation(format!(
                    "Failed to read upload file '{}': {}",
                    upload_path, e
                ))
            })?;

            request = request.query(&[("uploadType", "multipart")]);
            let (multipart_body, content_type) = build_multipart_body(&input.body, &file_bytes)?;
            request = request.header("Content-Type", content_type);
            request = request.body(multipart_body);
        } else if let Some(ref body_val) = input.body {
            request = request.header("Content-Type", "application/json");
            request = request.json(body_val);
        }
    }

    Ok(request)
}

/// Handle a JSON response: parse, sanitize via Model Armor, output, and check pagination.
/// Returns `Ok(true)` if the pagination loop should continue.
#[allow(clippy::too_many_arguments)]
async fn handle_json_response(
    body_text: &str,
    pagination: &PaginationConfig,
    sanitize_template: Option<&str>,
    sanitize_mode: &crate::helpers::modelarmor::SanitizeMode,
    output_format: &crate::formatter::OutputFormat,
    pages_fetched: &mut u32,
    page_token: &mut Option<String>,
    capture_output: bool,
    captured: &mut Vec<Value>,
) -> Result<bool, GwsError> {
    if let Ok(mut json_val) = serde_json::from_str::<Value>(body_text) {
        *pages_fetched += 1;

        // Run Model Armor sanitization if --sanitize is enabled
        if let Some(template) = sanitize_template {
            let text_to_check = serde_json::to_string(&json_val).unwrap_or_default();
            match crate::helpers::modelarmor::sanitize_text(template, &text_to_check).await {
                Ok(result) => {
                    let is_match = result.filter_match_state == "MATCH_FOUND";
                    if is_match {
                        eprintln!("⚠️  Model Armor: prompt injection detected (filterMatchState: MATCH_FOUND)");
                    }

                    if is_match && *sanitize_mode == crate::helpers::modelarmor::SanitizeMode::Block
                    {
                        let blocked = serde_json::json!({
                            "error": "Content blocked by Model Armor",
                            "sanitizationResult": serde_json::to_value(&result).unwrap_or_default(),
                        });
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&blocked).unwrap_or_default()
                        );
                        return Err(GwsError::Other(anyhow::anyhow!(
                            "Content blocked by Model Armor"
                        )));
                    }

                    if let Some(obj) = json_val.as_object_mut() {
                        obj.insert(
                            "_sanitization".to_string(),
                            serde_json::to_value(&result).unwrap_or_default(),
                        );
                    }
                }
                Err(e) => {
                    eprintln!("⚠️  Model Armor sanitization failed: {e}");
                }
            }
        }

        if capture_output {
            captured.push(json_val.clone());
        } else if pagination.page_all {
            let is_first_page = *pages_fetched == 1;
            println!(
                "{}",
                crate::formatter::format_value_paginated(&json_val, output_format, is_first_page)
            );
        } else {
            println!(
                "{}",
                crate::formatter::format_value(&json_val, output_format)
            );
        }

        // Check for nextPageToken to continue pagination
        if pagination.page_all {
            if let Some(next_token) = json_val.get("nextPageToken").and_then(|v| v.as_str()) {
                if *pages_fetched < pagination.page_limit {
                    *page_token = Some(next_token.to_string());
                    if pagination.page_delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            pagination.page_delay_ms,
                        ))
                        .await;
                    }
                    return Ok(true); // continue paginating
                }
            }
        }
    } else {
        // Not valid JSON, output as-is
        if !capture_output && !body_text.is_empty() {
            println!("{body_text}");
        }
    }

    Ok(false)
}

/// Handle a binary response by streaming it to a file.
async fn handle_binary_response(
    response: reqwest::Response,
    content_type: &str,
    output_path: Option<&str>,
    output_format: &crate::formatter::OutputFormat,
    capture_output: bool,
) -> Result<Option<Value>, GwsError> {
    let file_path = if let Some(p) = output_path {
        PathBuf::from(p)
    } else {
        let ext = mime_to_extension(content_type);
        PathBuf::from(format!("download.{ext}"))
    };

    let mut file = tokio::fs::File::create(&file_path)
        .await
        .context("Failed to create output file")?;

    let mut stream = response.bytes_stream();
    let mut total_bytes: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read response chunk")?;
        file.write_all(&chunk)
            .await
            .context("Failed to write to file")?;
        total_bytes += chunk.len() as u64;
    }

    file.flush().await.context("Failed to flush file")?;

    let result = json!({
        "status": "success",
        "saved_file": file_path.display().to_string(),
        "mimeType": content_type,
        "bytes": total_bytes,
    });

    if capture_output {
        return Ok(Some(result));
    }

    println!("{}", crate::formatter::format_value(&result, output_format));

    Ok(None)
}

/// Executes an API method call.
///
/// This is the core function of the CLI that handles:
/// 1. Parameter validation and URL construction.
/// 2. Request body validation against the Discovery Document schema.
/// 3. Authentication (OAuth or none).
/// 4. Sending the HTTP request (GET/POST/etc).
/// 5. Handling various response types (JSON, binary).
/// 6. Auto-pagination for list endpoints.
/// 7. Model Armor prompt injection scanning.
#[allow(clippy::too_many_arguments)]
pub async fn execute_method(
    doc: &RestDescription,
    method: &RestMethod,
    params_json: Option<&str>,
    body_json: Option<&str>,
    token: Option<&str>,
    auth_method: AuthMethod,
    output_path: Option<&str>,
    upload_path: Option<&str>,
    dry_run: bool,
    pagination: &PaginationConfig,
    sanitize_template: Option<&str>,
    sanitize_mode: &crate::helpers::modelarmor::SanitizeMode,
    output_format: &crate::formatter::OutputFormat,
    capture_output: bool,
) -> Result<Option<Value>, GwsError> {
    let input = parse_and_validate_inputs(doc, method, params_json, body_json, upload_path)?;

    if dry_run {
        let dry_run_info = json!({
            "dry_run": true,
            "url": input.full_url,
            "method": method.http_method,
            "query_params": input.query_params,
            "body": input.body,
            "is_multipart_upload": input.is_upload,
        });
        if capture_output {
            return Ok(Some(dry_run_info));
        }
        println!(
            "{}",
            crate::formatter::format_value(&dry_run_info, output_format)
        );
        return Ok(None);
    }

    let mut page_token: Option<String> = None;
    let mut pages_fetched: u32 = 0;
    let mut captured_values = Vec::new();

    loop {
        let client = crate::client::build_client()?;
        let request = build_http_request(
            &client,
            method,
            &input,
            token,
            &auth_method,
            page_token.as_deref(),
            pages_fetched,
            upload_path,
        )
        .await?;

        let response = request.send().await.context("HTTP request failed")?;

        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return handle_error_response(status, &error_body, &auth_method);
        }

        let is_json =
            content_type.contains("application/json") || content_type.contains("text/json");

        if is_json || content_type.is_empty() {
            let body_text = response
                .text()
                .await
                .context("Failed to read response body")?;

            let should_continue = handle_json_response(
                &body_text,
                pagination,
                sanitize_template,
                sanitize_mode,
                output_format,
                &mut pages_fetched,
                &mut page_token,
                capture_output,
                &mut captured_values,
            )
            .await?;

            if should_continue {
                continue;
            }
        } else if let Some(res) = handle_binary_response(
            response,
            &content_type,
            output_path,
            output_format,
            capture_output,
        )
        .await?
        {
            captured_values.push(res);
        }

        break;
    }

    if capture_output && !captured_values.is_empty() {
        if captured_values.len() == 1 {
            return Ok(Some(captured_values.pop().unwrap()));
        } else {
            return Ok(Some(Value::Array(captured_values)));
        }
    }

    Ok(None)
}

fn build_url(
    doc: &RestDescription,
    method: &RestMethod,
    params: &Map<String, Value>,
    is_upload: bool,
) -> Result<(String, HashMap<String, String>), GwsError> {
    // Build URL base and path

    // Actually we need to construct base URL properly if not present
    let base_url = if let Some(b) = &doc.base_url {
        b.clone()
    } else {
        format!("{}{}", doc.root_url, doc.service_path)
    };

    // Prefer flatPath when its placeholders match the method's path parameters.
    // Some Discovery Documents (e.g., Slides presentations.get) have flatPath
    // placeholders that don't match parameter names ({presentationsId} vs
    // {presentationId}). In those cases, fall back to path which uses RFC 6570
    // operators ({+var}) that this function already handles.
    let path_template = match method.flat_path.as_deref() {
        Some(fp) => {
            let all_match = method
                .parameters
                .iter()
                .filter(|(_, p)| p.location.as_deref() == Some("path"))
                .all(|(name, _)| {
                    let plain = format!("{{{name}}}");
                    let plus = format!("{{+{name}}}");
                    fp.contains(&plain) || fp.contains(&plus)
                });
            if all_match {
                fp
            } else {
                method.path.as_str()
            }
        }
        None => method.path.as_str(),
    };

    // Substitute path parameters and separate query parameters
    let path_parameters = extract_template_path_parameters(path_template);
    let mut query_params: HashMap<String, String> = HashMap::new();

    for (key, value) in params {
        let val_str = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };

        if path_parameters.contains(key.as_str()) {
            continue;
        }

        let is_path_param = method
            .parameters
            .get(key)
            .and_then(|p| p.location.as_deref())
            == Some("path");

        if is_path_param {
            return Err(GwsError::Validation(format!(
                "Path parameter '{}' was provided but is not present in URL template '{}'",
                key, path_template
            )));
        }

        // It's a query parameter
        query_params.insert(key.clone(), val_str);
    }

    let url_path = render_path_template(path_template, params)?;

    let full_url = if is_upload {
        // Use the upload endpoint from the Discovery Document
        let upload_endpoint = method
            .media_upload
            .as_ref()
            .and_then(|mu| mu.protocols.as_ref())
            .and_then(|p| p.simple.as_ref())
            .map(|s| s.path.as_str())
            .ok_or_else(|| {
                GwsError::Validation(
                    "Method supports media upload but no upload path found in Discovery Document"
                        .to_string(),
                )
            })?;
        let upload_path = render_path_template(upload_endpoint, params)?;
        format!("{}{}", doc.root_url.trim_end_matches('/'), upload_path)
    } else {
        format!("{base_url}{url_path}")
    };

    Ok((full_url, query_params))
}

fn extract_template_path_parameters(path_template: &str) -> HashSet<&str> {
    let mut found = HashSet::new();
    let mut cursor = 0;

    while let Some(open_idx) = path_template[cursor..].find('{') {
        let token_start = cursor + open_idx;
        let Some(close_idx) = path_template[token_start..].find('}') else {
            break;
        };

        let token_end = token_start + close_idx;
        let token = &path_template[token_start + 1..token_end];
        if let Some(key) = token.strip_prefix('+') {
            found.insert(key);
        } else {
            found.insert(token);
        }
        cursor = token_end + 1;
    }

    found
}

fn render_path_template(
    path_template: &str,
    params: &Map<String, Value>,
) -> Result<String, GwsError> {
    let mut rendered = String::with_capacity(path_template.len());
    let mut cursor = 0;

    while let Some(open_idx) = path_template[cursor..].find('{') {
        let token_start = cursor + open_idx;
        rendered.push_str(&path_template[cursor..token_start]);

        let Some(close_idx) = path_template[token_start..].find('}') else {
            rendered.push_str(&path_template[token_start..]);
            return Ok(rendered);
        };

        let token_end = token_start + close_idx;
        let token = &path_template[token_start + 1..token_end];
        let (is_plus, key) = if let Some(key) = token.strip_prefix('+') {
            (true, key)
        } else {
            (false, token)
        };

        if let Some(value) = params.get(key) {
            let val_str = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let encoded = if is_plus {
                let validated = crate::validate::validate_resource_name(&val_str)?;
                crate::validate::encode_path_preserving_slashes(validated)
            } else {
                crate::validate::encode_path_segment(&val_str)
            };
            rendered.push_str(&encoded);
        } else {
            rendered.push_str(&path_template[token_start..=token_end]);
        }

        cursor = token_end + 1;
    }

    rendered.push_str(&path_template[cursor..]);
    Ok(rendered)
}

/// Attempts to extract a GCP console enable URL from a Google API `accessNotConfigured`
/// error message.
///
/// The message format is typically:
/// `"<API> has not been used in project <N> before or it is disabled. Enable it by visiting <URL> then retry."`
///
/// Returns the URL string if found, otherwise `None`.
pub fn extract_enable_url(message: &str) -> Option<String> {
    // Look for "visiting <URL>" pattern
    let after_visiting = message.split("visiting ").nth(1)?;
    // URL ends at the next whitespace character
    let url = after_visiting
        .split_whitespace()
        .next()
        .map(|s| {
            s.trim_end_matches(|c: char| ['.', ',', ';', ':', ')', ']', '"', '\''].contains(&c))
        })
        .filter(|s| s.starts_with("http"))?;
    Some(url.to_string())
}

fn handle_error_response<T>(
    status: reqwest::StatusCode,
    error_body: &str,
    auth_method: &AuthMethod,
) -> Result<T, GwsError> {
    // If 401/403 and no auth was provided, give a helpful message
    if (status.as_u16() == 401 || status.as_u16() == 403) && *auth_method == AuthMethod::None {
        return Err(GwsError::Auth(
            "Access denied. No credentials provided. Run `gws auth login` or set \
             GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE to an OAuth credentials JSON file."
                .to_string(),
        ));
    }

    // Try to parse as Google API error
    if let Ok(error_json) = serde_json::from_str::<Value>(error_body) {
        if let Some(err_obj) = error_json.get("error") {
            let code = err_obj
                .get("code")
                .and_then(|c| c.as_u64())
                .unwrap_or(status.as_u16() as u64) as u16;
            let message = err_obj
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();

            // Reason can appear in "errors[0].reason" or at the top-level "reason" field.
            let reason = err_obj
                .get("errors")
                .and_then(|e| e.as_array())
                .and_then(|arr| arr.first())
                .and_then(|e| e.get("reason"))
                .and_then(|r| r.as_str())
                .or_else(|| err_obj.get("reason").and_then(|r| r.as_str()))
                .unwrap_or("unknown")
                .to_string();

            // For accessNotConfigured, extract the GCP enable URL from the message.
            let enable_url = if reason == "accessNotConfigured" {
                extract_enable_url(&message)
            } else {
                None
            };

            return Err(GwsError::Api {
                code,
                message,
                reason,
                enable_url,
            });
        }
    }

    Err(GwsError::Api {
        code: status.as_u16(),
        message: error_body.to_string(),
        reason: "httpError".to_string(),
        enable_url: None,
    })
}

/// Builds a multipart/related body for media upload requests.
///
/// Returns the body bytes and the Content-Type header value (with boundary).
fn build_multipart_body(
    metadata: &Option<Value>,
    file_bytes: &[u8],
) -> Result<(Vec<u8>, String), GwsError> {
    let boundary = format!("gws_boundary_{:016x}", rand::random::<u64>());

    // Determine the media MIME type from the metadata's mimeType field, or fall back
    let media_mime = metadata
        .as_ref()
        .and_then(|m| m.get("mimeType"))
        .and_then(|v| v.as_str())
        .unwrap_or("application/octet-stream");

    // Build multipart/related body
    let metadata_json = metadata
        .as_ref()
        .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());

    let mut body = Vec::new();
    // Part 1: JSON metadata
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
    body.extend_from_slice(metadata_json.as_bytes());
    body.extend_from_slice(b"\r\n");
    // Part 2: File content
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(format!("Content-Type: {media_mime}\r\n\r\n").as_bytes());
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(b"\r\n");
    // Closing boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let content_type = format!("multipart/related; boundary={boundary}");
    Ok((body, content_type))
}

/// Validates a JSON body against a Discovery Document schema.
fn validate_body_against_schema(
    body: &Value,
    schema_name: &str,
    doc: &RestDescription,
) -> Result<(), GwsError> {
    let mut errors = Vec::new();
    validate_value(body, schema_name, doc, "$", &mut errors);

    if !errors.is_empty() {
        return Err(GwsError::Validation(format!(
            "Request body failed schema validation:\n- {}",
            errors.join("\n- ")
        )));
    }

    Ok(())
}

fn validate_value(
    value: &Value,
    schema_ref_name: &str,
    doc: &RestDescription,
    path: &str,
    errors: &mut Vec<String>,
) {
    let schema = match doc.schemas.get(schema_ref_name) {
        Some(s) => s,
        None => {
            errors.push(format!("{path}: Schema '{schema_ref_name}' not found"));
            return;
        }
    };

    // If the top-level schema is an object
    if schema.schema_type.as_deref() == Some("object") || !schema.properties.is_empty() {
        if let Value::Object(obj) = value {
            validate_properties(obj, &schema.properties, &schema.required, doc, path, errors);
        } else {
            errors.push(format!("{path}: Expected object"));
        }
    }
}

fn validate_properties(
    obj: &Map<String, Value>,
    properties: &HashMap<String, crate::discovery::JsonSchemaProperty>,
    required_keys: &[String],
    doc: &RestDescription,
    path: &str,
    errors: &mut Vec<String>,
) {
    let valid_keys: std::collections::HashSet<&String> = properties.keys().collect();

    // Check required keys first
    for req_key in required_keys {
        if !obj.contains_key(req_key) {
            errors.push(format!("{path}: Missing required property '{req_key}'"));
        }
    }

    for (key, val) in obj {
        let current_path = if path == "$" {
            key.clone()
        } else {
            format!("{path}.{key}")
        };

        if !valid_keys.contains(key) {
            errors.push(format!(
                "{current_path}: Unknown property. Valid properties: {:?}",
                valid_keys.iter().map(|k| k.as_str()).collect::<Vec<_>>()
            ));
            continue;
        }

        let prop_schema = &properties[key];
        validate_property(val, prop_schema, doc, &current_path, errors);
    }
}

fn validate_property(
    value: &Value,
    prop_schema: &crate::discovery::JsonSchemaProperty,
    doc: &RestDescription,
    path: &str,
    errors: &mut Vec<String>,
) {
    // 1. Resolve $ref if present
    if let Some(ref_name) = &prop_schema.schema_ref {
        validate_value(value, ref_name, doc, path, errors);
        return;
    }

    // 2. Type checking
    if let Some(expected_type) = &prop_schema.prop_type {
        let type_matches = match (expected_type.as_str(), value) {
            ("string", Value::String(_)) => true,
            ("integer", Value::Number(n)) => n.is_i64() || n.is_u64(),
            ("number", Value::Number(_)) => true,
            ("boolean", Value::Bool(_)) => true,
            ("array", Value::Array(_)) => true,
            ("object", Value::Object(_)) => true,
            ("any", _) => true,
            _ => false,
        };

        if !type_matches {
            errors.push(format!(
                "{path}: Expected type '{expected_type}', found {}",
                get_value_type(value)
            ));
            return; // Stop further validation for this property if the type is wrong
        }
    }

    // 3. Array items validation
    if prop_schema.prop_type.as_deref() == Some("array") {
        if let Some(items_schema) = &prop_schema.items {
            if let Value::Array(arr) = value {
                for (i, item) in arr.iter().enumerate() {
                    let item_path = format!("{path}[{i}]");
                    validate_property(item, items_schema, doc, &item_path, errors);
                }
            }
        }
    }

    // 4. Object properties validation
    if prop_schema.prop_type.as_deref() == Some("object") && !prop_schema.properties.is_empty() {
        if let Value::Object(obj) = value {
            validate_properties(obj, &prop_schema.properties, &[], doc, path, errors);
        }
    }

    // 5. Enum validation
    if let Some(enum_values) = &prop_schema.enum_values {
        if let Value::String(s) = value {
            if !enum_values.contains(s) {
                errors.push(format!(
                    "{path}: Value '{s}' is not a valid enum member. Valid options: {:?}",
                    enum_values
                ));
            }
        }
    }
}

fn get_value_type(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number (float)",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Maps a MIME type to a file extension.
pub fn mime_to_extension(mime: &str) -> &str {
    if mime.contains("pdf") {
        "pdf"
    } else if mime.contains("png") {
        "png"
    } else if mime.contains("jpeg") || mime.contains("jpg") {
        "jpg"
    } else if mime.contains("gif") {
        "gif"
    } else if mime.contains("csv") {
        "csv"
    } else if mime.contains("zip") {
        "zip"
    } else if mime.contains("xml") {
        "xml"
    } else if mime.contains("html") {
        "html"
    } else if mime.contains("plain") {
        "txt"
    } else if mime.contains("octet-stream") {
        "bin"
    } else if mime.contains("spreadsheet") || mime.contains("xlsx") {
        "xlsx"
    } else if mime.contains("document") || mime.contains("docx") {
        "docx"
    } else if mime.contains("presentation") || mime.contains("pptx") {
        "pptx"
    } else if mime.contains("script") {
        "json"
    } else {
        "bin"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{JsonSchema, JsonSchemaProperty, RestDescription, RestMethod};
    use serde_json::json;

    #[test]
    fn test_pagination_config_default() {
        let config = PaginationConfig::default();
        assert_eq!(config.page_all, false);
        assert_eq!(config.page_limit, 10);
        assert_eq!(config.page_delay_ms, 100);
    }

    #[test]
    fn test_auth_method_equality() {
        assert_eq!(AuthMethod::OAuth, AuthMethod::OAuth);
        assert_eq!(AuthMethod::None, AuthMethod::None);
        assert_ne!(AuthMethod::OAuth, AuthMethod::None);
    }

    #[test]
    fn test_mime_to_extension_more_types() {
        assert_eq!(mime_to_extension("text/plain"), "txt");
        assert_eq!(mime_to_extension("text/csv"), "csv");
        assert_eq!(mime_to_extension("application/zip"), "zip");
        assert_eq!(mime_to_extension("application/xml"), "xml");
        assert_eq!(mime_to_extension("text/html"), "html");
        assert_eq!(mime_to_extension("application/json"), "bin"); // Default for unknown specific json types if not scripts
        assert_eq!(
            mime_to_extension("application/vnd.google-apps.script"),
            "json"
        );
        assert_eq!(
            mime_to_extension("application/vnd.google-apps.presentation"),
            "pptx"
        );
    }

    #[test]
    fn test_validate_body_valid() {
        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            JsonSchemaProperty {
                prop_type: Some("string".to_string()),
                ..Default::default()
            },
        );

        let mut schemas = HashMap::new();
        schemas.insert(
            "File".to_string(),
            JsonSchema {
                properties,
                ..Default::default()
            },
        );

        let doc = RestDescription {
            schemas,
            ..Default::default()
        };

        let body = json!({ "name": "My File" });
        assert!(validate_body_against_schema(&body, "File", &doc).is_ok());
    }

    #[test]
    fn test_validate_body_unknown_field() {
        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            JsonSchemaProperty {
                prop_type: Some("string".to_string()),
                ..Default::default()
            },
        );

        let mut schemas = HashMap::new();
        schemas.insert(
            "File".to_string(),
            JsonSchema {
                schema_type: Some("object".to_string()),
                properties,
                ..Default::default()
            },
        );

        let doc = RestDescription {
            schemas,
            ..Default::default()
        };

        let body = json!({ "name": "My File", "invalidField": 123 });
        let result = validate_body_against_schema(&body, "File", &doc);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown property"));
    }

    #[test]
    fn test_validate_body_deep_validation() {
        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            JsonSchemaProperty {
                prop_type: Some("string".to_string()),
                ..Default::default()
            },
        );
        properties.insert(
            "status".to_string(),
            JsonSchemaProperty {
                prop_type: Some("string".to_string()),
                enum_values: Some(vec!["ACTIVE".to_string(), "INACTIVE".to_string()]),
                ..Default::default()
            },
        );
        properties.insert(
            "count".to_string(),
            JsonSchemaProperty {
                prop_type: Some("integer".to_string()),
                ..Default::default()
            },
        );
        properties.insert(
            "tags".to_string(),
            JsonSchemaProperty {
                prop_type: Some("array".to_string()),
                items: Some(Box::new(JsonSchemaProperty {
                    prop_type: Some("string".to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            },
        );
        properties.insert(
            "parent".to_string(),
            JsonSchemaProperty {
                schema_ref: Some("Parent".to_string()),
                ..Default::default()
            },
        );

        let mut parent_props = HashMap::new();
        parent_props.insert(
            "id".to_string(),
            JsonSchemaProperty {
                prop_type: Some("string".to_string()),
                ..Default::default()
            },
        );

        let mut schemas = HashMap::new();
        schemas.insert(
            "File".to_string(),
            JsonSchema {
                schema_type: Some("object".to_string()),
                required: vec!["name".to_string(), "status".to_string()],
                properties,
                ..Default::default()
            },
        );
        schemas.insert(
            "Parent".to_string(),
            JsonSchema {
                schema_type: Some("object".to_string()),
                properties: parent_props,
                ..Default::default()
            },
        );

        let doc = RestDescription {
            schemas,
            ..Default::default()
        };

        // Valid Request
        let body = json!({
            "name": "My File",
            "status": "ACTIVE",
            "count": 10,
            "tags": ["one", "two"],
            "parent": { "id": "123" }
        });
        assert!(validate_body_against_schema(&body, "File", &doc).is_ok());

        // Missing Required Field
        let body_missing = json!({ "name": "My File" });
        let err = validate_body_against_schema(&body_missing, "File", &doc).unwrap_err();
        assert!(err
            .to_string()
            .contains("Missing required property 'status'"));

        // Invalid Enum Value
        let body_bad_enum = json!({ "name": "My File", "status": "UNKNOWN" });
        let err = validate_body_against_schema(&body_bad_enum, "File", &doc).unwrap_err();
        assert!(err.to_string().contains("not a valid enum member"));

        // Invalid Type
        let body_bad_type = json!({ "name": "My File", "status": "ACTIVE", "count": "10" });
        let err = validate_body_against_schema(&body_bad_type, "File", &doc).unwrap_err();
        assert!(err
            .to_string()
            .contains("Expected type 'integer', found string"));

        // Deep Schema Reference Validation Failure
        let body_bad_ref = json!({
            "name": "My File",
            "status": "ACTIVE",
            "parent": { "invalidField": "123" }
        });
        let err = validate_body_against_schema(&body_bad_ref, "File", &doc).unwrap_err();
        assert!(err.to_string().contains("Unknown property"));

        // Expected Object Type Failure
        let body_not_object = json!([]);
        let err = validate_body_against_schema(&body_not_object, "File", &doc).unwrap_err();
        assert!(err.to_string().contains("Expected object"));
    }
    #[tokio::test]
    async fn test_build_multipart_body() {
        let metadata = Some(json!({ "name": "test.txt", "mimeType": "text/plain" }));
        let content = b"Hello world";

        let (body, content_type) = build_multipart_body(&metadata, content).unwrap();

        // Check content type has boundary
        assert!(content_type.starts_with("multipart/related; boundary="));
        let boundary = content_type.split("boundary=").nth(1).unwrap();

        let body_str = String::from_utf8(body).unwrap();

        // Verify structure
        assert!(body_str.contains(boundary));
        assert!(body_str.contains("Content-Type: application/json"));
        assert!(body_str.contains("{\"mimeType\":\"text/plain\",\"name\":\"test.txt\"}"));
        assert!(body_str.contains("Content-Type: text/plain"));
        assert!(body_str.contains("Hello world"));
    }

    #[tokio::test]
    async fn test_build_multipart_body_no_metadata() {
        let metadata = None;
        let content = b"Binary data";

        let (body, content_type) = build_multipart_body(&metadata, content).unwrap();
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let body_str = String::from_utf8(body).unwrap();

        assert!(body_str.contains(boundary));
        assert!(body_str.contains("application/octet-stream")); // Fallback mime
        assert!(body_str.contains("Binary data"));
    }

    #[test]
    fn test_build_url_basic() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let method = RestMethod {
            path: "files".to_string(),
            flat_path: Some("files".to_string()),
            ..Default::default()
        };
        let params = Map::new();

        let (url, _) = build_url(&doc, &method, &params, false).unwrap();
        assert_eq!(url, "https://api.example.com/files");
    }

    #[test]
    fn test_build_url_substitution() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let method = RestMethod {
            path: "files/{fileId}".to_string(),
            flat_path: Some("files/{fileId}".to_string()),
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("fileId".to_string(), json!("123"));

        let (url, _) = build_url(&doc, &method, &params, false).unwrap();
        assert_eq!(url, "https://api.example.com/files/123");
    }

    #[test]
    fn test_build_url_query_params() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let method = RestMethod {
            path: "files".to_string(),
            flat_path: Some("files".to_string()),
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("q".to_string(), json!("search term"));

        let (url, query) = build_url(&doc, &method, &params, false).unwrap();
        assert_eq!(url, "https://api.example.com/files");
        assert_eq!(query.get("q").unwrap(), "search term");
    }

    #[test]
    fn test_build_url_encodes_path_parameter_chars() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let mut parameters = HashMap::new();
        parameters.insert(
            "spreadsheetId".to_string(),
            crate::discovery::MethodParameter {
                location: Some("path".to_string()),
                ..Default::default()
            },
        );
        parameters.insert(
            "range".to_string(),
            crate::discovery::MethodParameter {
                location: Some("path".to_string()),
                ..Default::default()
            },
        );
        let method = RestMethod {
            path: "spreadsheets/{spreadsheetId}/values/{range}".to_string(),
            flat_path: Some("spreadsheets/{spreadsheetId}/values/{range}".to_string()),
            parameters,
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("spreadsheetId".to_string(), json!("abc123"));
        params.insert("range".to_string(), json!("hash#1!A1:B2"));

        let (url, _) = build_url(&doc, &method, &params, false).unwrap();
        assert_eq!(
            url,
            "https://api.example.com/spreadsheets/abc123/values/hash%231%21A1%3AB2"
        );
    }

    #[test]
    fn test_build_url_plus_expansion_preserves_slashes() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let mut parameters = HashMap::new();
        parameters.insert(
            "name".to_string(),
            crate::discovery::MethodParameter {
                location: Some("path".to_string()),
                ..Default::default()
            },
        );
        let method = RestMethod {
            path: "v1/{+name}".to_string(),
            flat_path: Some("v1/{+name}".to_string()),
            parameters,
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert(
            "name".to_string(),
            json!("projects/p1/locations/us/topics/t1"),
        );

        let (url, _) = build_url(&doc, &method, &params, false).unwrap();
        assert_eq!(
            url,
            "https://api.example.com/v1/projects/p1/locations/us/topics/t1"
        );
    }

    #[test]
    fn test_build_url_plus_expansion_rejects_reserved_chars() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let mut parameters = HashMap::new();
        parameters.insert(
            "name".to_string(),
            crate::discovery::MethodParameter {
                location: Some("path".to_string()),
                ..Default::default()
            },
        );
        let method = RestMethod {
            path: "v1/{+name}".to_string(),
            flat_path: Some("v1/{+name}".to_string()),
            parameters,
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("name".to_string(), json!("projects/p1#frag?x=y"));

        let err = build_url(&doc, &method, &params, false).unwrap_err();
        assert!(err.to_string().contains("must not contain '?' or '#'"));
    }

    #[test]
    fn test_build_url_plus_expansion_rejects_path_traversal() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let mut parameters = HashMap::new();
        parameters.insert(
            "name".to_string(),
            crate::discovery::MethodParameter {
                location: Some("path".to_string()),
                ..Default::default()
            },
        );
        let method = RestMethod {
            path: "v1/{+name}".to_string(),
            flat_path: Some("v1/{+name}".to_string()),
            parameters,
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("name".to_string(), json!("projects/../../etc/passwd"));

        let err = build_url(&doc, &method, &params, false).unwrap_err();
        assert!(err.to_string().contains("path traversal"));
    }

    #[test]
    fn test_build_url_upload_endpoint_substitutes_path_params() {
        let doc = RestDescription {
            root_url: "https://www.googleapis.com/".to_string(),
            ..Default::default()
        };
        let mut parameters = HashMap::new();
        parameters.insert(
            "fileId".to_string(),
            crate::discovery::MethodParameter {
                location: Some("path".to_string()),
                ..Default::default()
            },
        );
        let method = RestMethod {
            path: "drive/v3/files/{fileId}".to_string(),
            flat_path: Some("drive/v3/files/{fileId}".to_string()),
            parameters,
            media_upload: Some(crate::discovery::MediaUpload {
                protocols: Some(crate::discovery::MediaUploadProtocols {
                    simple: Some(crate::discovery::MediaUploadProtocol {
                        path: "/upload/drive/v3/files/{fileId}".to_string(),
                        multipart: Some(true),
                    }),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut params = Map::new();
        params.insert("fileId".to_string(), json!("abc/123"));

        let (url, _) = build_url(&doc, &method, &params, true).unwrap();
        assert_eq!(
            url,
            "https://www.googleapis.com/upload/drive/v3/files/abc%2F123"
        );
    }

    #[test]
    fn test_build_url_does_not_replace_placeholder_like_values() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let method = RestMethod {
            path: "v1/{parent}/{child}".to_string(),
            flat_path: Some("v1/{parent}/{child}".to_string()),
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("parent".to_string(), json!("literal-{child}-value"));
        params.insert("child".to_string(), json!("ok"));

        let (url, _) = build_url(&doc, &method, &params, false).unwrap();
        assert_eq!(
            url,
            "https://api.example.com/v1/literal%2D%7Bchild%7D%2Dvalue/ok"
        );
    }

    #[test]
    fn test_build_url_errors_for_path_param_not_in_template() {
        let doc = RestDescription {
            base_url: Some("https://api.example.com/".to_string()),
            ..Default::default()
        };
        let mut parameters = HashMap::new();
        parameters.insert(
            "fileId".to_string(),
            crate::discovery::MethodParameter {
                location: Some("path".to_string()),
                ..Default::default()
            },
        );
        let method = RestMethod {
            path: "files".to_string(),
            flat_path: Some("files".to_string()),
            parameters,
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("fileId".to_string(), json!("123"));

        let err = build_url(&doc, &method, &params, false).unwrap_err();
        assert!(err
            .to_string()
            .contains("Path parameter 'fileId' was provided but is not present"));
    }

    #[test]
    fn test_build_url_flatpath_fallback_on_mismatch() {
        // Reproduces the Slides presentations.get bug where flatPath uses
        // {presentationsId} (plural) but the parameter is presentationId (singular).
        let doc = RestDescription {
            base_url: Some("https://slides.googleapis.com/".to_string()),
            ..Default::default()
        };
        let mut parameters = HashMap::new();
        parameters.insert(
            "presentationId".to_string(),
            crate::discovery::MethodParameter {
                location: Some("path".to_string()),
                required: true,
                ..Default::default()
            },
        );
        let method = RestMethod {
            path: "v1/presentations/{+presentationId}".to_string(),
            flat_path: Some("v1/presentations/{presentationsId}".to_string()),
            parameters,
            ..Default::default()
        };
        let mut params = Map::new();
        params.insert("presentationId".to_string(), json!("abc123"));

        let (url, _) = build_url(&doc, &method, &params, false).unwrap();
        assert_eq!(url, "https://slides.googleapis.com/v1/presentations/abc123");
    }

    #[test]
    fn test_handle_error_response_401() {
        let err = handle_error_response::<()>(
            reqwest::StatusCode::UNAUTHORIZED,
            "Unauthorized",
            &AuthMethod::None,
        )
        .unwrap_err();
        match err {
            GwsError::Auth(msg) => assert!(msg.contains("Access denied")),
            _ => panic!("Expected Auth error"),
        }
    }

    #[test]
    fn test_handle_error_response_api_error() {
        let json_err = json!({
            "error": {
                "code": 400,
                "message": "Bad Request",
                "errors": [{ "reason": "bad" }]
            }
        })
        .to_string();

        let err = handle_error_response::<()>(
            reqwest::StatusCode::BAD_REQUEST,
            &json_err,
            &AuthMethod::OAuth,
        )
        .unwrap_err();
        match err {
            GwsError::Api {
                code,
                message,
                reason,
                ..
            } => {
                assert_eq!(code, 400);
                assert_eq!(message, "Bad Request");
                assert_eq!(reason, "bad");
            }
            _ => panic!("Expected Api error"),
        }
    }
}

#[tokio::test]
async fn test_execute_method_dry_run() {
    let mut schemas = HashMap::new();
    let mut properties = HashMap::new();
    properties.insert(
        "name".to_string(),
        crate::discovery::JsonSchemaProperty {
            prop_type: Some("string".to_string()),
            ..Default::default()
        },
    );
    schemas.insert(
        "File".to_string(),
        crate::discovery::JsonSchema {
            schema_type: Some("object".to_string()),
            properties,
            ..Default::default()
        },
    );

    let doc = RestDescription {
        root_url: "https://example.googleapis.com/".to_string(),
        service_path: "v1/".to_string(),
        schemas,
        ..Default::default()
    };

    let mut parameters = HashMap::new();
    parameters.insert(
        "fileId".to_string(),
        crate::discovery::MethodParameter {
            location: Some("path".to_string()),
            required: true,
            ..Default::default()
        },
    );

    let method = RestMethod {
        http_method: "POST".to_string(),
        id: Some("example.files.create".to_string()),
        path: "files/{fileId}".to_string(),
        parameter_order: vec!["fileId".to_string()],
        parameters,
        request: Some(crate::discovery::SchemaRef {
            schema_ref: Some("File".to_string()),
            parameter_name: None,
        }),
        ..Default::default()
    };

    let params_json = r#"{"fileId": "123"}"#;
    let body_json = r#"{"name": "test.txt"}"#;

    let sanitize_mode = crate::helpers::modelarmor::SanitizeMode::Warn;
    let pagination = PaginationConfig::default();

    let result = execute_method(
        &doc,
        &method,
        Some(params_json),
        Some(body_json),
        None,
        AuthMethod::None,
        None,
        None,
        true, // dry_run
        &pagination,
        None,
        &sanitize_mode,
        &crate::formatter::OutputFormat::default(),
        false,
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_method_missing_path_param() {
    // Same setup but missing required fileId in params
    let mut parameters = HashMap::new();
    parameters.insert(
        "fileId".to_string(),
        crate::discovery::MethodParameter {
            location: Some("path".to_string()),
            required: true,
            ..Default::default()
        },
    );
    let doc = RestDescription::default();
    let method = RestMethod {
        http_method: "POST".to_string(),
        path: "files/{fileId}".to_string(),
        parameter_order: vec!["fileId".to_string()],
        parameters,
        ..Default::default()
    };

    let sanitize_mode = crate::helpers::modelarmor::SanitizeMode::Warn;
    let result = execute_method(
        &doc,
        &method,
        None, // No params provided
        None,
        None,
        AuthMethod::None,
        None,
        None,
        true,
        &PaginationConfig::default(),
        None,
        &sanitize_mode,
        &crate::formatter::OutputFormat::default(),
        false,
    )
    .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Required path parameter"));
}

#[test]
fn test_handle_error_response_non_json() {
    let err = handle_error_response::<()>(
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "Internal Server Error Text",
        &AuthMethod::OAuth,
    )
    .unwrap_err();
    match err {
        GwsError::Api {
            code,
            message,
            reason,
            ..
        } => {
            assert_eq!(code, 500);
            assert_eq!(message, "Internal Server Error Text");
            assert_eq!(reason, "httpError");
        }
        _ => panic!("Expected Api error"),
    }
}

#[test]
fn test_extract_enable_url_typical_message() {
    let msg = "Gmail API has not been used in project 549352339482 before or it is disabled. \
               Enable it by visiting https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482 then retry.";
    let url = extract_enable_url(msg);
    assert_eq!(
        url.as_deref(),
        Some("https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482")
    );
}

#[test]
fn test_extract_enable_url_no_url() {
    let msg = "API not enabled.";
    assert_eq!(extract_enable_url(msg), None);
}

#[test]
fn test_extract_enable_url_non_http() {
    let msg = "Enable it by visiting ftp://example.com then retry.";
    assert_eq!(extract_enable_url(msg), None);
}

#[test]
fn test_extract_enable_url_trims_trailing_punctuation() {
    let msg = "Enable it by visiting https://console.cloud.google.com/apis/library?project=test123. Then retry.";
    let url = extract_enable_url(msg);
    assert_eq!(
        url.as_deref(),
        Some("https://console.cloud.google.com/apis/library?project=test123")
    );
}

#[test]
fn test_handle_error_response_access_not_configured_with_url() {
    // Matches the top-level "reason" field format Google actually returns for this error
    let json_err = serde_json::json!({
        "error": {
            "code": 403,
            "message": "Gmail API has not been used in project 549352339482 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482 then retry.",
            "status": "PERMISSION_DENIED",
            "reason": "accessNotConfigured"
        }
    })
    .to_string();

    let err = handle_error_response::<()>(
        reqwest::StatusCode::FORBIDDEN,
        &json_err,
        &AuthMethod::OAuth,
    )
    .unwrap_err();

    match err {
        GwsError::Api {
            code,
            reason,
            enable_url,
            ..
        } => {
            assert_eq!(code, 403);
            assert_eq!(reason, "accessNotConfigured");
            assert_eq!(
                enable_url.as_deref(),
                Some("https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482")
            );
        }
        _ => panic!("Expected Api error"),
    }
}

#[test]
fn test_handle_error_response_access_not_configured_errors_array() {
    // Some Google APIs put reason in errors[0].reason
    let json_err = serde_json::json!({
        "error": {
            "code": 403,
            "message": "Drive API has not been used in project 12345 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/drive.googleapis.com/overview?project=12345 then retry.",
            "errors": [{ "reason": "accessNotConfigured" }]
        }
    })
    .to_string();

    let err = handle_error_response::<()>(
        reqwest::StatusCode::FORBIDDEN,
        &json_err,
        &AuthMethod::OAuth,
    )
    .unwrap_err();

    match err {
        GwsError::Api {
            reason, enable_url, ..
        } => {
            assert_eq!(reason, "accessNotConfigured");
            assert!(enable_url.is_some());
            assert!(enable_url.unwrap().contains("drive.googleapis.com"));
        }
        _ => panic!("Expected Api error"),
    }
}

#[test]
fn test_get_value_type_helper() {
    assert_eq!(get_value_type(&json!(null)), "null");
    assert_eq!(get_value_type(&json!(true)), "boolean");
    assert_eq!(get_value_type(&json!(42)), "integer");
    assert_eq!(get_value_type(&json!(3.5)), "number (float)");
    assert_eq!(get_value_type(&json!("string")), "string");
    assert_eq!(get_value_type(&json!([1, 2])), "array");
    assert_eq!(get_value_type(&json!({"a": 1})), "object");
}

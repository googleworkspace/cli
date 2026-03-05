#![allow(dead_code)]
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

//! Discovery Document Parsing and Management
//!
//! Handles fetching, caching, and parsing Google API Discovery Documents.
//! These JSON schemas define the shapes of API requests and responses, forming
//! the foundation of the dynamically generated CLI commands.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level Discovery REST Description document.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestDescription {
    pub name: String,
    pub version: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub root_url: String,
    #[serde(default)]
    pub service_path: String,
    pub base_url: Option<String>,
    #[serde(default)]
    pub schemas: HashMap<String, JsonSchema>,
    #[serde(default)]
    pub resources: HashMap<String, RestResource>,
    #[serde(default)]
    pub parameters: HashMap<String, MethodParameter>,
    pub auth: Option<AuthDescription>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AuthDescription {
    pub oauth2: Option<OAuth2Description>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OAuth2Description {
    pub scopes: Option<HashMap<String, ScopeDescription>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ScopeDescription {
    pub description: Option<String>,
}

/// A resource in the Discovery Document, which can contain methods and nested sub-resources.
#[derive(Debug, Deserialize, Default)]
pub struct RestResource {
    #[serde(default)]
    pub methods: HashMap<String, RestMethod>,
    #[serde(default)]
    pub resources: HashMap<String, RestResource>,
}

/// A single API method.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestMethod {
    pub id: Option<String>,
    pub description: Option<String>,
    pub http_method: String,
    pub path: String,
    #[serde(default)]
    pub parameters: HashMap<String, MethodParameter>,
    #[serde(default)]
    pub parameter_order: Vec<String>,
    pub request: Option<SchemaRef>,
    pub response: Option<SchemaRef>,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub flat_path: Option<String>,
    #[serde(default)]
    pub supports_media_download: bool,
    #[serde(default)]
    pub supports_media_upload: bool,
    pub media_upload: Option<MediaUpload>,
}

/// Media upload metadata from the Discovery Document.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUpload {
    pub protocols: Option<MediaUploadProtocols>,
    pub accept: Option<Vec<String>>,
}

/// Upload protocol details.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUploadProtocols {
    pub simple: Option<MediaUploadProtocol>,
}

/// A single upload protocol entry.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUploadProtocol {
    pub path: String,
    pub multipart: Option<bool>,
}

/// A reference to a schema (e.g., `{ "$ref": "File" }`).
#[derive(Debug, Deserialize, Default)]
pub struct SchemaRef {
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    #[serde(rename = "parameterName")]
    pub parameter_name: Option<String>,
}

/// A parameter definition for a method.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MethodParameter {
    #[serde(rename = "type")]
    pub param_type: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub format: Option<String>,
    pub default: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub enum_descriptions: Option<Vec<String>>,
    #[serde(default)]
    pub repeated: bool,
    pub minimum: Option<String>,
    pub maximum: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
}

/// JSON Schema definition for request/response bodies.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchema {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub schema_type: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub properties: HashMap<String, JsonSchemaProperty>,
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    pub items: Option<Box<JsonSchemaProperty>>,
    #[serde(default)]
    pub required: Vec<String>,
    pub additional_properties: Option<Box<JsonSchemaProperty>>,
}

/// A property within a JSON Schema.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchemaProperty {
    #[serde(rename = "type")]
    pub prop_type: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    pub format: Option<String>,
    pub items: Option<Box<JsonSchemaProperty>>,
    #[serde(default)]
    pub properties: HashMap<String, JsonSchemaProperty>,
    #[serde(default)]
    pub read_only: bool,
    pub default: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub additional_properties: Option<Box<JsonSchemaProperty>>,
}

/// Cached custom API base URL, read once from the environment.
/// Prints a warning on first access so the redirect is never silent.
static CUSTOM_API_BASE_URL: std::sync::LazyLock<Option<String>> = std::sync::LazyLock::new(|| {
    let url = std::env::var("GOOGLE_WORKSPACE_CLI_API_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty());
    if let Some(ref u) = url {
        eprintln!("[gws] Custom API endpoint active: {u}");
        eprintln!("[gws] Authentication is disabled. Requests will NOT go to Google APIs.");
    }
    url
});

/// Returns the custom API base URL override, if set.
///
/// When `GOOGLE_WORKSPACE_CLI_API_BASE_URL` is set (e.g., `http://localhost:8099`), all API
/// requests are directed to this endpoint instead of the real Google APIs.
/// Authentication is skipped automatically. This is useful for testing against
/// mock API servers.
pub fn custom_api_base_url() -> Option<&'static str> {
    CUSTOM_API_BASE_URL.as_deref()
}

/// Fetches and caches a Google Discovery Document.
///
/// The Discovery Document is always fetched from the real Google APIs so that
/// gws knows the full command structure (resources, methods, parameters). When
/// `GOOGLE_WORKSPACE_CLI_API_BASE_URL` is set, the document's `root_url` and `base_url` are
/// rewritten to point at the custom endpoint — actual API requests then go to
/// the mock server while the CLI command tree remains fully functional.
pub async fn fetch_discovery_document(
    service: &str,
    version: &str,
) -> anyhow::Result<RestDescription> {
    // Validate service and version to prevent path traversal in cache filenames
    // and injection in discovery URLs.
    let service =
        crate::validate::validate_api_identifier(service).map_err(|e| anyhow::anyhow!("{e}"))?;
    let version =
        crate::validate::validate_api_identifier(version).map_err(|e| anyhow::anyhow!("{e}"))?;

    let cache_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("gws")
        .join("cache");
    std::fs::create_dir_all(&cache_dir)?;

    let cache_file = cache_dir.join(format!("{service}_{version}.json"));

    // Check cache (24hr TTL)
    if cache_file.exists() {
        if let Ok(metadata) = std::fs::metadata(&cache_file) {
            if let Ok(modified) = metadata.modified() {
                if modified.elapsed().unwrap_or_default() < std::time::Duration::from_secs(86400) {
                    let data = std::fs::read_to_string(&cache_file)?;
                    let mut doc: RestDescription = serde_json::from_str(&data)?;
                    apply_base_url_override(&mut doc);
                    return Ok(doc);
                }
            }
        }
    }

    let url = format!(
        "https://www.googleapis.com/discovery/v1/apis/{}/{}/rest",
        crate::validate::encode_path_segment(service),
        crate::validate::encode_path_segment(version),
    );

    let client = crate::client::build_client()?;
    let resp = client.get(&url).send().await?;

    let body = if resp.status().is_success() {
        resp.text().await?
    } else {
        // Try the $discovery/rest URL pattern used by newer APIs (Forms, Keep, Meet, etc.)
        let alt_url = format!("https://{service}.googleapis.com/$discovery/rest");
        let alt_resp = client
            .get(&alt_url)
            .query(&[("version", version)])
            .send()
            .await?;
        if !alt_resp.status().is_success() {
            anyhow::bail!(
                "Failed to fetch Discovery Document for {service}/{version}: HTTP {} (tried both standard and $discovery URLs)",
                alt_resp.status()
            );
        }
        alt_resp.text().await?
    };

    // Write to cache
    if let Err(e) = std::fs::write(&cache_file, &body) {
        // Non-fatal: just warn via stderr-safe approach
        let _ = e;
    }

    let mut doc: RestDescription = serde_json::from_str(&body)?;
    apply_base_url_override(&mut doc);
    Ok(doc)
}

/// Rewrite Discovery Document URLs when `GOOGLE_WORKSPACE_CLI_API_BASE_URL` is set.
/// Uses the same base_url structure as the original document — just
/// swaps the host so request paths stay correct for the mock server.
fn apply_base_url_override(doc: &mut RestDescription) {
    if let Some(base) = custom_api_base_url() {
        rewrite_base_url(doc, base);
    }
}

/// Rewrites `root_url` and `base_url` in a Discovery Document to point at a
/// custom endpoint while preserving the service path (e.g., `drive/v3/`).
/// Extracted for testability (the env-var path goes through `LazyLock` which
/// is hard to toggle in tests).
fn rewrite_base_url(doc: &mut RestDescription, base: &str) {
    let base_trimmed = base.trim_end_matches('/');
    let new_root_url = format!("{base_trimmed}/");
    let original_root_url = std::mem::replace(&mut doc.root_url, new_root_url);

    if let Some(base_url) = &mut doc.base_url {
        *base_url = base_url.replace(&original_root_url, &doc.root_url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_rest_description() {
        let json = r#"{
            "name": "drive",
            "version": "v3",
            "rootUrl": "https://www.googleapis.com/",
            "servicePath": "drive/v3/",
            "resources": {
                "files": {
                    "methods": {
                        "list": {
                            "httpMethod": "GET",
                            "path": "files",
                            "response": { "$ref": "FileList" }
                        }
                    }
                }
            },
            "schemas": {
                "FileList": {
                    "id": "FileList",
                    "type": "object",
                    "properties": {
                        "files": {
                            "type": "array",
                            "items": { "$ref": "File" }
                        }
                    }
                }
            }
        }"#;

        let doc: RestDescription = serde_json::from_str(json).unwrap();
        assert_eq!(doc.name, "drive");
        assert_eq!(doc.version, "v3");
        assert_eq!(doc.root_url, "https://www.googleapis.com/");
        assert_eq!(doc.service_path, "drive/v3/");

        // precise resource checking
        let files = doc.resources.get("files").expect("files resource missing");
        let list = files.methods.get("list").expect("list method missing");
        assert_eq!(list.http_method, "GET");
        assert_eq!(list.path, "files");

        // schema checking
        let file_list = doc
            .schemas
            .get("FileList")
            .expect("FileList schema missing");
        assert_eq!(file_list.id.as_deref(), Some("FileList"));
    }

    #[test]
    fn test_deserialize_defaults() {
        let json = r#"{
            "name": "admin",
            "version": "directory_v1",
            "rootUrl": "https://admin.googleapis.com/"
        }"#;

        let doc: RestDescription = serde_json::from_str(json).unwrap();
        assert_eq!(doc.service_path, ""); // default empty string
        assert!(doc.resources.is_empty());
        assert!(doc.schemas.is_empty());
    }

    #[test]
    fn test_rewrite_base_url_empty_service_path() {
        // Gmail-style: rootUrl includes the host, servicePath is empty,
        // method paths include the full path (e.g., "gmail/v1/users/{userId}/profile")
        let mut doc = RestDescription {
            name: "gmail".to_string(),
            version: "v1".to_string(),
            root_url: "https://gmail.googleapis.com/".to_string(),
            base_url: Some("https://gmail.googleapis.com/".to_string()),
            service_path: "".to_string(),
            ..Default::default()
        };

        rewrite_base_url(&mut doc, "http://localhost:8099");
        assert_eq!(doc.root_url, "http://localhost:8099/");
        assert_eq!(doc.base_url.as_deref(), Some("http://localhost:8099/"));
    }

    #[test]
    fn test_rewrite_base_url_preserves_service_path() {
        // Drive-style: rootUrl is the shared host, servicePath is "drive/v3/",
        // method paths are short (e.g., "files")
        let mut doc = RestDescription {
            name: "drive".to_string(),
            version: "v3".to_string(),
            root_url: "https://www.googleapis.com/".to_string(),
            base_url: Some("https://www.googleapis.com/drive/v3/".to_string()),
            service_path: "drive/v3/".to_string(),
            ..Default::default()
        };

        rewrite_base_url(&mut doc, "http://localhost:8099/");
        assert_eq!(doc.root_url, "http://localhost:8099/");
        assert_eq!(
            doc.base_url.as_deref(),
            Some("http://localhost:8099/drive/v3/")
        );
    }

    #[test]
    fn test_rewrite_base_url_none() {
        // Some Discovery Documents omit base_url; build_url() falls back to
        // root_url + service_path in that case. Verify we don't panic.
        let mut doc = RestDescription {
            name: "customsearch".to_string(),
            version: "v1".to_string(),
            root_url: "https://www.googleapis.com/".to_string(),
            base_url: None,
            service_path: "customsearch/v1/".to_string(),
            ..Default::default()
        };

        rewrite_base_url(&mut doc, "http://localhost:8099");
        assert_eq!(doc.root_url, "http://localhost:8099/");
        assert!(doc.base_url.is_none());
    }
}

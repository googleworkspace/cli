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

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use anyhow::Context;
use clap::{Arg, ArgMatches, Command};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::future::Future;
use std::io::Write;
use std::pin::Pin;

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const DEFAULT_LOCATION: &str = "us-central1";
const DEFAULT_PUBLISHER: &str = "google";

pub struct VertexHelper;

/// Build the regional Vertex AI endpoint URL.
/// Vertex AI requires region-prefixed hosts: {location}-aiplatform.googleapis.com
fn regional_base_url(location: &str) -> String {
    format!("https://{location}-aiplatform.googleapis.com/v1")
}

/// Resolve the GCP project ID from flag or environment.
fn resolve_project(matches: &ArgMatches) -> Result<String, GwsError> {
    if let Some(p) = matches.get_one::<String>("project") {
        let validated = crate::validate::validate_resource_name(p)?;
        return Ok(validated.to_string());
    }
    if let Ok(p) = std::env::var("GOOGLE_WORKSPACE_PROJECT_ID") {
        if !p.is_empty() {
            let validated = crate::validate::validate_resource_name(&p)?;
            return Ok(validated.to_string());
        }
    }
    Err(GwsError::Validation(
        "Project ID required. Use --project or set GOOGLE_WORKSPACE_PROJECT_ID.".to_string(),
    ))
}

/// Build the model resource path for the Vertex AI endpoint.
/// Accepts either a short model name (e.g. "gemini-2.0-flash") which gets
/// expanded to the full resource path, or a full resource name passed through.
pub fn build_model_resource(
    project: &str,
    location: &str,
    model: &str,
    publisher: &str,
) -> String {
    if model.starts_with("projects/") {
        model.to_string()
    } else {
        format!("projects/{project}/locations/{location}/publishers/{publisher}/models/{model}")
    }
}

/// Build the full URL for a generateContent or streamGenerateContent call.
pub fn build_generate_url(location: &str, model_resource: &str, stream: bool) -> String {
    let base = regional_base_url(location);
    let method = if stream {
        "streamGenerateContent"
    } else {
        "generateContent"
    };
    let encoded = crate::validate::encode_path_preserving_slashes(model_resource);
    format!("{base}/{encoded}:{method}")
}

/// Build the JSON request body from ergonomic CLI flags.
pub fn build_request_body(matches: &ArgMatches) -> Result<String, GwsError> {
    if let Some(raw) = matches.get_one::<String>("json") {
        serde_json::from_str::<Value>(raw)
            .map_err(|e| GwsError::Validation(format!("Invalid --json body: {e}")))?;
        return Ok(raw.clone());
    }

    let text = if let Some(t) = matches.get_one::<String>("text") {
        t.clone()
    } else {
        let stdin_text =
            std::io::read_to_string(std::io::stdin()).context("Failed to read stdin")?;
        if stdin_text.trim().is_empty() {
            return Err(GwsError::Validation(
                "Provide a prompt via --text, --json, or pipe to stdin.".to_string(),
            ));
        }
        stdin_text.trim().to_string()
    };

    let mut body = json!({
        "contents": [{"role": "user", "parts": [{"text": text}]}],
    });

    if let Some(system) = matches.get_one::<String>("system-instruction") {
        body["systemInstruction"] = json!({
            "parts": [{"text": system}]
        });
    }

    let mut gen_config = serde_json::Map::new();
    if let Some(temp) = matches.get_one::<f64>("temperature") {
        gen_config.insert("temperature".into(), json!(temp));
    }
    if let Some(max) = matches.get_one::<u32>("max-tokens") {
        gen_config.insert("maxOutputTokens".into(), json!(max));
    }
    if let Some(top_p) = matches.get_one::<f64>("top-p") {
        gen_config.insert("topP".into(), json!(top_p));
    }
    if let Some(top_k) = matches.get_one::<u32>("top-k") {
        gen_config.insert("topK".into(), json!(top_k));
    }
    if !gen_config.is_empty() {
        body["generationConfig"] = Value::Object(gen_config);
    }

    Ok(body.to_string())
}

/// Extract the text content from a Vertex AI generateContent response.
pub fn extract_text(response: &Value) -> Option<String> {
    response
        .get("candidates")?
        .as_array()?
        .first()?
        .get("content")?
        .get("parts")?
        .as_array()?
        .iter()
        .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .first()
        .map(|s| s.to_string())
}

/// Extract the location from a full model resource name if present.
/// e.g. "projects/p/locations/us-central1/publishers/google/models/gemini-2.0-flash"
pub fn extract_location_from_model(model: &str) -> Option<&str> {
    let parts: Vec<&str> = model.split('/').collect();
    for i in 0..parts.len() {
        if parts[i] == "locations" && i + 1 < parts.len() {
            return Some(parts[i + 1]);
        }
    }
    None
}

fn build_generate_cmd(name: &str, about: &str, after_help: &str) -> Command {
    Command::new(name)
        .about(about)
        .arg(
            Arg::new("model")
                .long("model")
                .short('m')
                .help("Model name (e.g. gemini-2.0-flash) or full resource path")
                .required(true)
                .value_name("MODEL"),
        )
        .arg(
            Arg::new("project")
                .long("project")
                .help("GCP project ID (falls back to GOOGLE_WORKSPACE_PROJECT_ID)")
                .value_name("PROJECT"),
        )
        .arg(
            Arg::new("location")
                .long("location")
                .help("GCP region (default: us-central1)")
                .value_name("LOCATION"),
        )
        .arg(
            Arg::new("publisher")
                .long("publisher")
                .help("Model publisher (default: google)")
                .value_name("PUBLISHER"),
        )
        .arg(
            Arg::new("text")
                .long("text")
                .short('t')
                .help("Text prompt to send")
                .value_name("TEXT"),
        )
        .arg(
            Arg::new("system-instruction")
                .long("system-instruction")
                .help("System instruction for the model")
                .value_name("TEXT"),
        )
        .arg(
            Arg::new("temperature")
                .long("temperature")
                .help("Sampling temperature (0.0–2.0)")
                .value_name("FLOAT")
                .value_parser(clap::value_parser!(f64)),
        )
        .arg(
            Arg::new("max-tokens")
                .long("max-tokens")
                .help("Maximum output tokens")
                .value_name("N")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("top-p")
                .long("top-p")
                .help("Nucleus sampling threshold")
                .value_name("FLOAT")
                .value_parser(clap::value_parser!(f64)),
        )
        .arg(
            Arg::new("top-k")
                .long("top-k")
                .help("Top-k sampling parameter")
                .value_name("N")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .help("Full JSON request body (overrides all other content flags)")
                .value_name("JSON"),
        )
        .arg(
            Arg::new("raw")
                .long("raw")
                .help("Output the full JSON response instead of extracted text")
                .action(clap::ArgAction::SetTrue),
        )
        .after_help(after_help)
}

impl Helper for VertexHelper {
    fn inject_commands(&self, mut cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd = cmd.subcommand(build_generate_cmd(
            "+generate",
            "[Helper] Generate content using a Vertex AI Gemini model",
            "\
EXAMPLES:
  gws vertex +generate --model gemini-2.0-flash --text 'Explain Rust in one sentence'
  gws vertex +generate -m gemini-2.5-pro -t 'Write a haiku' --temperature 0.9
  echo 'Summarize this' | gws vertex +generate -m gemini-2.0-flash --project my-proj
  gws vertex +generate -m gemini-2.0-flash --json '{\"contents\":[{\"parts\":[{\"text\":\"Hi\"}]}]}'

TIPS:
  --project defaults to GOOGLE_WORKSPACE_PROJECT_ID env var.
  --location defaults to us-central1.
  Short model names like 'gemini-2.0-flash' are expanded automatically.
  Use --raw to see the full API response JSON.",
        ));
        cmd = cmd.subcommand(build_generate_cmd(
            "+stream-generate",
            "[Helper] Stream content from a Vertex AI Gemini model",
            "\
EXAMPLES:
  gws vertex +stream-generate --model gemini-2.0-flash --text 'Tell me a story'
  gws vertex +stream-generate -m gemini-2.5-pro -t 'Explain quantum computing' --raw

TIPS:
  Text is printed incrementally as the model generates it.
  Use --raw to output each streamed JSON chunk on a separate line (NDJSON).",
        ));
        cmd
    }

    fn helper_only(&self) -> bool {
        true
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a super::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(sub) = matches.subcommand_matches("+generate") {
                handle_generate(sub, false).await?;
                return Ok(true);
            }
            if let Some(sub) = matches.subcommand_matches("+stream-generate") {
                handle_generate(sub, true).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

/// Resolve common parameters from CLI flags.
struct ResolvedParams {
    project: String,
    location: String,
    publisher: String,
    model_resource: String,
    url: String,
    body: String,
}

fn resolve_params(matches: &ArgMatches, stream: bool) -> Result<ResolvedParams, GwsError> {
    let model_raw = matches.get_one::<String>("model").unwrap();
    let model = crate::validate::validate_resource_name(model_raw)?;

    let location_from_model = extract_location_from_model(model).map(|s| s.to_string());
    let location = matches
        .get_one::<String>("location")
        .cloned()
        .or(location_from_model)
        .unwrap_or_else(|| DEFAULT_LOCATION.to_string());
    let location_validated = crate::validate::validate_resource_name(&location)?;

    let publisher = matches
        .get_one::<String>("publisher")
        .cloned()
        .unwrap_or_else(|| DEFAULT_PUBLISHER.to_string());

    let project = resolve_project(matches)?;
    let model_resource = build_model_resource(&project, location_validated, model, &publisher);
    let url = build_generate_url(location_validated, &model_resource, stream);
    let body = build_request_body(matches)?;

    Ok(ResolvedParams {
        project,
        location: location_validated.to_string(),
        publisher,
        model_resource,
        url,
        body,
    })
}

async fn handle_generate(matches: &ArgMatches, stream: bool) -> Result<(), GwsError> {
    let params = resolve_params(matches, stream)?;
    let raw_output = matches.get_flag("raw");

    let token = auth::get_token(&[CLOUD_PLATFORM_SCOPE])
        .await
        .context("Failed to get auth token for Vertex AI")?;

    let client = crate::client::build_client()?;

    let mut request = client
        .post(&params.url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(params.body);

    if let Some(quota_project) = auth::get_quota_project() {
        request = request.header("x-goog-user-project", quota_project);
    }

    if stream {
        request = request.query(&[("alt", "sse")]);
    }

    let response = request.send().await.context("Vertex AI request failed")?;
    let status = response.status();

    if !status.is_success() {
        let err_text = response
            .text()
            .await
            .context("Failed to read error response")?;
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: err_text,
            reason: "vertexAiError".to_string(),
            enable_url: None,
        });
    }

    if stream {
        handle_stream_response(response, raw_output).await
    } else {
        handle_unary_response(response, raw_output).await
    }
}

async fn handle_unary_response(
    response: reqwest::Response,
    raw: bool,
) -> Result<(), GwsError> {
    let text = response
        .text()
        .await
        .context("Failed to read response body")?;

    if raw {
        println!("{text}");
        return Ok(());
    }

    let json: Value =
        serde_json::from_str(&text).context("Failed to parse Vertex AI response")?;

    if let Some(content) = extract_text(&json) {
        println!("{content}");
    } else {
        println!("{}", serde_json::to_string_pretty(&json).unwrap_or(text));
    }

    Ok(())
}

async fn handle_stream_response(
    response: reqwest::Response,
    raw: bool,
) -> Result<(), GwsError> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("Failed to read stream chunk")?;
        let text = String::from_utf8_lossy(&bytes);
        buffer.push_str(&text);

        while let Some(data_start) = buffer.find("data: ") {
            let after_data = &buffer[data_start + 6..];
            let line_end = after_data.find('\n').unwrap_or(after_data.len());
            let data_line = &after_data[..line_end];

            if data_line.trim() == "[DONE]" {
                buffer = buffer[data_start + 6 + line_end..].to_string();
                continue;
            }

            match serde_json::from_str::<Value>(data_line) {
                Ok(event_json) => {
                    if raw {
                        let _ = writeln!(handle, "{}", serde_json::to_string(&event_json).unwrap_or_default());
                    } else if let Some(content) = extract_text(&event_json) {
                        let _ = write!(handle, "{content}");
                        let _ = handle.flush();
                    }
                    buffer = buffer[data_start + 6 + line_end..].to_string();
                }
                Err(_) => {
                    // Incomplete JSON — wait for more data
                    break;
                }
            }
        }
    }

    if !raw {
        let _ = writeln!(handle);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regional_base_url() {
        assert_eq!(
            regional_base_url("us-central1"),
            "https://us-central1-aiplatform.googleapis.com/v1"
        );
        assert_eq!(
            regional_base_url("europe-west4"),
            "https://europe-west4-aiplatform.googleapis.com/v1"
        );
    }

    #[test]
    fn test_build_model_resource_short_name() {
        let res = build_model_resource("my-proj", "us-central1", "gemini-2.0-flash", "google");
        assert_eq!(
            res,
            "projects/my-proj/locations/us-central1/publishers/google/models/gemini-2.0-flash"
        );
    }

    #[test]
    fn test_build_model_resource_full_name_passthrough() {
        let full = "projects/p/locations/eu/publishers/google/models/gemini-2.5-pro";
        let res = build_model_resource("ignored", "ignored", full, "ignored");
        assert_eq!(res, full);
    }

    #[test]
    fn test_build_generate_url_unary() {
        let model = "projects/p/locations/us-central1/publishers/google/models/gemini-2.0-flash";
        let url = build_generate_url("us-central1", model, false);
        assert!(url.starts_with("https://us-central1-aiplatform.googleapis.com/v1/"));
        assert!(url.ends_with(":generateContent"));
    }

    #[test]
    fn test_build_generate_url_stream() {
        let model = "projects/p/locations/us-central1/publishers/google/models/gemini-2.0-flash";
        let url = build_generate_url("us-central1", model, true);
        assert!(url.ends_with(":streamGenerateContent"));
    }

    #[test]
    fn test_extract_text_valid() {
        let resp = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello world"}]
                }
            }]
        });
        assert_eq!(extract_text(&resp), Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_text_no_candidates() {
        let resp = json!({"error": "something"});
        assert_eq!(extract_text(&resp), None);
    }

    #[test]
    fn test_extract_text_empty_parts() {
        let resp = json!({
            "candidates": [{
                "content": {"role": "model", "parts": []}
            }]
        });
        assert_eq!(extract_text(&resp), None);
    }

    #[test]
    fn test_extract_location_from_model_present() {
        let model = "projects/p/locations/europe-west4/publishers/google/models/gemini-2.0-flash";
        assert_eq!(extract_location_from_model(model), Some("europe-west4"));
    }

    #[test]
    fn test_extract_location_from_model_short_name() {
        assert_eq!(extract_location_from_model("gemini-2.0-flash"), None);
    }

    #[test]
    fn test_build_request_body_from_text_flag() {
        let cmd = Command::new("test")
            .arg(Arg::new("text").long("text"))
            .arg(Arg::new("json").long("json"))
            .arg(Arg::new("system-instruction").long("system-instruction"))
            .arg(
                Arg::new("temperature")
                    .long("temperature")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("max-tokens")
                    .long("max-tokens")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                Arg::new("top-p")
                    .long("top-p")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("top-k")
                    .long("top-k")
                    .value_parser(clap::value_parser!(u32)),
            );
        let matches = cmd
            .try_get_matches_from(["test", "--text", "hello world"])
            .unwrap();
        let body = build_request_body(&matches).unwrap();
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["contents"][0]["parts"][0]["text"], "hello world");
    }

    #[test]
    fn test_build_request_body_with_generation_config() {
        let cmd = Command::new("test")
            .arg(Arg::new("text").long("text"))
            .arg(Arg::new("json").long("json"))
            .arg(Arg::new("system-instruction").long("system-instruction"))
            .arg(
                Arg::new("temperature")
                    .long("temperature")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("max-tokens")
                    .long("max-tokens")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                Arg::new("top-p")
                    .long("top-p")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("top-k")
                    .long("top-k")
                    .value_parser(clap::value_parser!(u32)),
            );
        let matches = cmd
            .try_get_matches_from([
                "test",
                "--text",
                "hello",
                "--temperature",
                "0.7",
                "--max-tokens",
                "1024",
            ])
            .unwrap();
        let body = build_request_body(&matches).unwrap();
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["generationConfig"]["temperature"], 0.7);
        assert_eq!(json["generationConfig"]["maxOutputTokens"], 1024);
    }

    #[test]
    fn test_build_request_body_with_system_instruction() {
        let cmd = Command::new("test")
            .arg(Arg::new("text").long("text"))
            .arg(Arg::new("json").long("json"))
            .arg(Arg::new("system-instruction").long("system-instruction"))
            .arg(
                Arg::new("temperature")
                    .long("temperature")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("max-tokens")
                    .long("max-tokens")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                Arg::new("top-p")
                    .long("top-p")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("top-k")
                    .long("top-k")
                    .value_parser(clap::value_parser!(u32)),
            );
        let matches = cmd
            .try_get_matches_from([
                "test",
                "--text",
                "hello",
                "--system-instruction",
                "You are a poet",
            ])
            .unwrap();
        let body = build_request_body(&matches).unwrap();
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["systemInstruction"]["parts"][0]["text"], "You are a poet");
    }

    #[test]
    fn test_build_request_body_raw_json_passthrough() {
        let cmd = Command::new("test")
            .arg(Arg::new("text").long("text"))
            .arg(Arg::new("json").long("json"))
            .arg(Arg::new("system-instruction").long("system-instruction"))
            .arg(
                Arg::new("temperature")
                    .long("temperature")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("max-tokens")
                    .long("max-tokens")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                Arg::new("top-p")
                    .long("top-p")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("top-k")
                    .long("top-k")
                    .value_parser(clap::value_parser!(u32)),
            );
        let raw = r#"{"contents":[{"parts":[{"text":"raw"}]}]}"#;
        let matches = cmd
            .try_get_matches_from(["test", "--json", raw])
            .unwrap();
        let body = build_request_body(&matches).unwrap();
        assert_eq!(body, raw);
    }

    #[test]
    fn test_build_request_body_invalid_json_rejected() {
        let cmd = Command::new("test")
            .arg(Arg::new("text").long("text"))
            .arg(Arg::new("json").long("json"))
            .arg(Arg::new("system-instruction").long("system-instruction"))
            .arg(
                Arg::new("temperature")
                    .long("temperature")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("max-tokens")
                    .long("max-tokens")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                Arg::new("top-p")
                    .long("top-p")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("top-k")
                    .long("top-k")
                    .value_parser(clap::value_parser!(u32)),
            );
        let matches = cmd
            .try_get_matches_from(["test", "--json", "not valid json"])
            .unwrap();
        assert!(build_request_body(&matches).is_err());
    }

    #[test]
    fn test_inject_commands() {
        let helper = VertexHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();
        let cmd = helper.inject_commands(cmd, &doc);
        let subs: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subs.contains(&"+generate"));
        assert!(subs.contains(&"+stream-generate"));
    }

    #[test]
    fn test_helper_only_is_true() {
        let helper = VertexHelper;
        assert!(helper.helper_only());
    }

    #[test]
    fn test_resolve_project_rejects_traversal() {
        let cmd = Command::new("test").arg(Arg::new("project").long("project"));
        let matches = cmd
            .try_get_matches_from(["test", "--project", "../etc"])
            .unwrap();
        assert!(resolve_project(&matches).is_err());
    }
}

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
use crate::discovery::RestDescription;
use crate::error::GwsError;
use anyhow::Context;
use clap::{Arg, ArgMatches, Command};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;

/// Result of a Model Armor sanitization check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SanitizationResult {
    /// The overall state of the match (e.g., "MATCH_FOUND", "NO_MATCH_FOUND").
    pub filter_match_state: String,
    /// Detailed results from specific filters (PI, Jailbreak, etc.).
    #[serde(default)]
    pub filter_results: serde_json::Value,
    /// The final decision based on the policy (e.g., "BLOCK", "ALLOW").
    #[serde(default)]
    pub invocation_result: String,
}

/// Controls behavior when sanitization finds a match.
#[derive(Debug, Clone, PartialEq)]
pub enum SanitizeMode {
    /// Log warning to stderr, annotate output with _sanitization field
    Warn,
    /// Suppress response output, exit non-zero
    Block,
}

/// Configuration for Model Armor sanitization, threaded through the CLI.
#[derive(Debug, Clone)]
pub struct SanitizeConfig {
    pub template: Option<String>,
    pub mode: SanitizeMode,
}

impl Default for SanitizeConfig {
    /// Provides default values for `SanitizeConfig`.
    ///
    /// By default, no template is set (sanitization disabled) and the mode is `Warn`.
    fn default() -> Self {
        Self {
            template: None,
            mode: SanitizeMode::Warn,
        }
    }
}

impl SanitizeMode {
    /// Parses a string into a `SanitizeMode`.
    ///
    /// * "block" (case-insensitive) -> `Block`
    /// * Any other value -> `Warn` (safe default)
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "block" => SanitizeMode::Block,
            _ => SanitizeMode::Warn,
        }
    }
}

pub struct ModelArmorHelper;

/// Build the regional base URL for Model Armor API.
/// The discovery doc rootUrl (modelarmor.us.rep.googleapis.com) is incorrect —
/// Model Armor requires region-specific endpoints: modelarmor.{region}.rep.googleapis.com
fn regional_base_url(location: &str) -> String {
    format!("https://modelarmor.{location}.rep.googleapis.com/v1")
}

/// Extract location from a full template resource name.
/// e.g. "projects/my-project/locations/us-central1/templates/my-template" -> "us-central1"
fn extract_location(resource_name: &str) -> Option<&str> {
    let parts: Vec<&str> = resource_name.split('/').collect();
    for i in 0..parts.len() {
        if parts[i] == "locations" && i + 1 < parts.len() {
            return Some(parts[i + 1]);
        }
    }
    None
}

impl Helper for ModelArmorHelper {
    fn inject_commands(&self, mut cmd: Command, _doc: &RestDescription) -> Command {
        cmd = cmd.subcommand(
            Command::new("+sanitize-prompt")
                .about("[Helper] Sanitize a user prompt through a Model Armor template")
                .arg(
                    Arg::new("template")
                        .long("template")
                        .help("Full template resource name (projects/PROJECT/locations/LOCATION/templates/TEMPLATE)")
                        .required(true)
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("text")
                        .long("text")
                        .help("Text content to sanitize")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("json")
                        .long("json")
                        .help("Full JSON request body (overrides --text)")
                        .value_name("JSON"),
                )
                .after_help("\
EXAMPLES:
  gws modelarmor +sanitize-prompt --template projects/P/locations/L/templates/T --text 'user input'
  echo 'prompt' | gws modelarmor +sanitize-prompt --template ...

TIPS:
  If neither --text nor --json is given, reads from stdin.
  For outbound safety, use +sanitize-response instead."),
        );

        cmd = cmd.subcommand(
            Command::new("+sanitize-response")
                .about("[Helper] Sanitize a model response through a Model Armor template")
                .arg(
                    Arg::new("template")
                        .long("template")
                        .help("Full template resource name (projects/PROJECT/locations/LOCATION/templates/TEMPLATE)")
                        .required(true)
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("text")
                        .long("text")
                        .help("Text content to sanitize")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("json")
                        .long("json")
                        .help("Full JSON request body (overrides --text)")
                        .value_name("JSON"),
                )
                .after_help("\
EXAMPLES:
  gws modelarmor +sanitize-response --template projects/P/locations/L/templates/T --text 'model output'
  model_cmd | gws modelarmor +sanitize-response --template ...

TIPS:
  Use for outbound safety (model -> user).
  For inbound safety (user -> model), use +sanitize-prompt."),
        );

        cmd = cmd.subcommand(
            Command::new("+create-template")
                .about("[Helper] Create a new Model Armor template")
                .arg(
                    Arg::new("project")
                        .long("project")
                        .help("GCP project ID")
                        .required(true)
                        .value_name("PROJECT"),
                )
                .arg(
                    Arg::new("location")
                        .long("location")
                        .help("GCP location (e.g. us-central1)")
                        .required(true)
                        .value_name("LOCATION"),
                )
                .arg(
                    Arg::new("template-id")
                        .long("template-id")
                        .help("Template ID to create")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("preset")
                        .long("preset")
                        .help("Use a preset template: jailbreak")
                        .value_name("PRESET")
                        .value_parser(["jailbreak"]),
                )
                .arg(
                    Arg::new("json")
                        .long("json")
                        .help("JSON body for the template configuration (overrides --preset)")
                        .value_name("JSON"),
                )
                .after_help("\
EXAMPLES:
  gws modelarmor +create-template --project P --location us-central1 --template-id my-tmpl --preset jailbreak
  gws modelarmor +create-template --project P --location us-central1 --template-id my-tmpl --json '{...}'

TIPS:
  Defaults to the jailbreak preset if neither --preset nor --json is given.
  Use the resulting template name with +sanitize-prompt and +sanitize-response."),
        );

        cmd
    }

    fn helper_only(&self) -> bool {
        true
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(sub) = matches.subcommand_matches("+sanitize-prompt") {
                handle_sanitize(sub, "sanitizeUserPrompt", "userPromptData").await?;
                return Ok(true);
            }
            if let Some(sub) = matches.subcommand_matches("+sanitize-response") {
                handle_sanitize(sub, "sanitizeModelResponse", "modelResponseData").await?;
                return Ok(true);
            }
            if let Some(sub) = matches.subcommand_matches("+create-template") {
                handle_create_template(sub).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

pub const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Sanitize text through a Model Armor template and return the result.
/// Template format: projects/PROJECT/locations/LOCATION/templates/TEMPLATE
pub async fn sanitize_text(template: &str, text: &str) -> Result<SanitizationResult, GwsError> {
    let (body, url) = build_sanitize_request_data(template, text, "sanitizeUserPrompt")?;

    let token = auth::get_token(&[CLOUD_PLATFORM_SCOPE], None)
        .await
        .context("Failed to get auth token for Model Armor")?;

    let client = crate::client::build_client()?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .context("Model Armor request failed")?;

    let status = resp.status();
    let resp_text = resp
        .text()
        .await
        .context("Failed to read Model Armor response")?;

    if !status.is_success() {
        return Err(GwsError::Other(anyhow::anyhow!(
            "Model Armor API returned status {status}: {resp_text}"
        )));
    }

    parse_sanitize_response(&resp_text)
}

/// Make a POST request to Model Armor's regional API endpoint.
async fn model_armor_post(url: &str, body: &str) -> Result<(), GwsError> {
    let token = auth::get_token(&[CLOUD_PLATFORM_SCOPE], None)
        .await
        .context("Failed to get auth token")?;

    let client = crate::client::build_client()?;
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .context("HTTP request failed")?;

    let status = resp.status();
    let text = resp.text().await.context("Failed to read response")?;

    println!("{text}");

    if !status.is_success() {
        return Err(GwsError::Other(anyhow::anyhow!(
            "API returned status {status}"
        )));
    }

    Ok(())
}

/// Handle +sanitize-prompt and +sanitize-response
async fn handle_sanitize(
    matches: &ArgMatches,
    method_name: &str,
    data_field: &str,
) -> Result<(), GwsError> {
    let template_raw = matches.get_one::<String>("template").unwrap();
    let template = crate::validate::validate_resource_name(template_raw)?;

    let location = extract_location(template).ok_or_else(|| {
        GwsError::Validation(
            "Cannot extract location from template name. Expected format: projects/PROJECT/locations/LOCATION/templates/TEMPLATE".to_string(),
        )
    })?;

    let body = parse_sanitize_args(matches, data_field)?;

    let base = regional_base_url(location);
    let url = format!("{base}/{template}:{method_name}");

    model_armor_post(&url, &body).await
}

#[derive(Debug, PartialEq)]
pub struct CreateTemplateConfig {
    pub project: String,
    pub location: String,
    pub template_id: String,
    pub body: String,
}

fn parse_create_template_args(matches: &ArgMatches) -> Result<CreateTemplateConfig, GwsError> {
    let project_raw = matches.get_one::<String>("project").unwrap();
    let project = crate::validate::validate_resource_name(project_raw)?.to_string();
    let location_raw = matches.get_one::<String>("location").unwrap();
    let location = crate::validate::validate_resource_name(location_raw)?.to_string();
    let template_id_raw = matches.get_one::<String>("template-id").unwrap();
    let template_id = crate::validate::validate_resource_name(template_id_raw)?.to_string();

    let body = if let Some(json_str) = matches.get_one::<String>("json") {
        json_str.clone()
    } else {
        let preset = matches
            .get_one::<String>("preset")
            .map(|s| s.as_str())
            .unwrap_or("jailbreak");
        load_preset_template(preset)?
    };

    Ok(CreateTemplateConfig {
        project,
        location,
        template_id,
        body,
    })
}

pub fn build_create_template_url(config: &CreateTemplateConfig) -> String {
    let base = regional_base_url(&config.location);
    let project = crate::validate::encode_path_segment(&config.project);
    let location = crate::validate::encode_path_segment(&config.location);
    let parent = format!("projects/{project}/locations/{location}");
    format!(
        "{base}/{parent}/templates?templateId={}",
        crate::validate::encode_path_segment(&config.template_id)
    )
}

/// Handle +create-template
async fn handle_create_template(matches: &ArgMatches) -> Result<(), GwsError> {
    let config = parse_create_template_args(matches)?;
    let url = build_create_template_url(&config);

    eprintln!(
        "Creating template '{}' with preset: {}",
        config.template_id,
        matches
            .get_one::<String>("preset")
            .map(|s| s.as_str())
            .unwrap_or("jailbreak")
    );

    model_armor_post(&url, &config.body).await
}

/// Loads a preset template JSON file from the templates/modelarmor/ directory.
/// Falls back to the embedded template if the file is not found.
fn load_preset_template(name: &str) -> Result<String, GwsError> {
    // Try to find templates relative to the executable
    let exe_path = std::env::current_exe().ok();
    let search_dirs: Vec<std::path::PathBuf> = [
        // Relative to current directory
        Some(std::path::PathBuf::from("templates/modelarmor")),
        // Relative to executable
        exe_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("../templates/modelarmor")),
        exe_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("templates/modelarmor")),
    ]
    .into_iter()
    .flatten()
    .collect();

    let filename = format!("{name}.json");

    for dir in &search_dirs {
        let path = dir.join(&filename);
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read template '{}'", path.display()))?;
            eprintln!("Using preset template from: {}", path.display());
            return Ok(content);
        }
    }

    // Fallback: embedded preset
    eprintln!("Template file not found, using embedded '{}' preset", name);
    Ok(include_str!("../../templates/modelarmor/jailbreak.json").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_config_default() {
        let config = SanitizeConfig::default();
        assert!(config.template.is_none());
        assert_eq!(config.mode, SanitizeMode::Warn);
    }

    #[test]
    fn test_sanitize_config_with_template() {
        let config = SanitizeConfig {
            template: Some("projects/p/locations/us-central1/templates/t".to_string()),
            mode: SanitizeMode::Block,
        };
        assert_eq!(
            config.template.as_deref(),
            Some("projects/p/locations/us-central1/templates/t")
        );
        assert_eq!(config.mode, SanitizeMode::Block);
    }

    #[test]
    fn test_sanitize_mode_from_str_warn() {
        assert_eq!(SanitizeMode::from_str("warn"), SanitizeMode::Warn);
        assert_eq!(SanitizeMode::from_str("WARN"), SanitizeMode::Warn);
        assert_eq!(SanitizeMode::from_str("Warn"), SanitizeMode::Warn);
    }

    #[test]
    fn test_sanitize_mode_from_str_block() {
        assert_eq!(SanitizeMode::from_str("block"), SanitizeMode::Block);
        assert_eq!(SanitizeMode::from_str("BLOCK"), SanitizeMode::Block);
        assert_eq!(SanitizeMode::from_str("Block"), SanitizeMode::Block);
    }

    #[test]
    fn test_sanitize_mode_from_str_unknown_defaults_to_warn() {
        assert_eq!(SanitizeMode::from_str(""), SanitizeMode::Warn);
        assert_eq!(SanitizeMode::from_str("invalid"), SanitizeMode::Warn);
        assert_eq!(SanitizeMode::from_str("stop"), SanitizeMode::Warn);
    }

    #[test]
    fn test_extract_location_valid() {
        assert_eq!(
            extract_location("projects/my-project/locations/us-central1/templates/my-template"),
            Some("us-central1")
        );
    }

    #[test]
    fn test_extract_location_different_region() {
        assert_eq!(
            extract_location("projects/p/locations/europe-west1/templates/t"),
            Some("europe-west1")
        );
    }

    #[test]
    fn test_extract_location_no_locations() {
        assert_eq!(extract_location("projects/my-project/templates/t"), None);
    }

    #[test]
    fn test_extract_location_empty() {
        assert_eq!(extract_location(""), None);
    }

    #[test]
    fn test_extract_location_trailing_locations() {
        // "locations" at the end with no value after
        assert_eq!(extract_location("projects/p/locations"), None);
    }

    #[test]
    fn test_regional_base_url() {
        assert_eq!(
            regional_base_url("us-central1"),
            "https://modelarmor.us-central1.rep.googleapis.com/v1"
        );
    }

    #[test]
    fn test_regional_base_url_different_region() {
        assert_eq!(
            regional_base_url("europe-west1"),
            "https://modelarmor.europe-west1.rep.googleapis.com/v1"
        );
    }

    #[test]
    fn test_cloud_platform_scope_constant() {
        assert_eq!(
            CLOUD_PLATFORM_SCOPE,
            "https://www.googleapis.com/auth/cloud-platform"
        );
    }

    #[test]
    fn test_build_sanitize_request_data() {
        let template = "projects/p/locations/us-central1/templates/t";
        let (body, _) =
            build_sanitize_request_data(template, "some text", "sanitizeUserPrompt").unwrap();
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["userPromptData"]["text"], "some text");
    }

    #[test]
    fn test_parse_sanitize_response_success() {
        let json_resp = json!({
            "sanitizationResult": {
                "filterMatchState": "MATCH_FOUND",
                "filterResults": {},
                "invocationResult": "SUCCESS"
            }
        })
        .to_string();

        let res = parse_sanitize_response(&json_resp).unwrap();
        assert_eq!(res.filter_match_state, "MATCH_FOUND");
    }

    #[test]
    fn test_parse_sanitize_response_missing_field() {
        let json_resp = json!({}).to_string();
        assert!(parse_sanitize_response(&json_resp).is_err());
    }
}

pub fn build_sanitize_request_data(
    template: &str,
    text: &str,
    method: &str,
) -> Result<(String, String), GwsError> {
    let location = extract_location(template).ok_or_else(|| {
        GwsError::Validation(
            "Cannot extract location from --sanitize template. Expected format: projects/PROJECT/locations/LOCATION/templates/TEMPLATE".to_string(),
        )
    })?;

    let base = regional_base_url(location);
    let url = format!("{base}/{template}:{method}");

    // Identify data field based on method
    let data_field = if method == "sanitizeUserPrompt" {
        "userPromptData"
    } else {
        "modelResponseData"
    };

    let body = json!({data_field: {"text": text}}).to_string();
    Ok((body, url))
}

pub fn parse_sanitize_response(resp_text: &str) -> Result<SanitizationResult, GwsError> {
    // Parse the response to extract sanitizationResult
    let parsed: serde_json::Value =
        serde_json::from_str(resp_text).context("Failed to parse Model Armor response")?;

    let result = parsed.get("sanitizationResult").ok_or_else(|| {
        GwsError::Other(anyhow::anyhow!(
            "No sanitizationResult in Model Armor response"
        ))
    })?;

    let res =
        serde_json::from_value(result.clone()).context("Failed to parse sanitization result")?;
    Ok(res)
}

fn parse_sanitize_args(matches: &ArgMatches, data_field: &str) -> Result<String, GwsError> {
    if let Some(json_str) = matches.get_one::<String>("json") {
        Ok(json_str.clone())
    } else if let Some(text) = matches.get_one::<String>("text") {
        let mut body = serde_json::Map::new();
        body.insert(data_field.to_string(), json!({"text": text}));
        Ok(serde_json::Value::Object(body).to_string())
    } else {
        // Try to read from stdin, but since we can't easily test stdin in unit tests,
        // we might check for TTY or empty stdin.
        // For simplicity here, we assume if we reach here without text/json, we try stdin.

        // Note: We removed the TTY check to avoid adding 'atty' or 'is-terminal' dependency.
        // This means it will block on stdin if no input is provided, which is standard CLI behavior.

        let stdin_text =
            std::io::read_to_string(std::io::stdin()).context("Failed to read stdin")?;

        if stdin_text.trim().is_empty() {
            return Err(GwsError::Validation(
                "Provide text via --text, --json, or pipe to stdin".to_string(),
            ));
        }
        let mut body = serde_json::Map::new();
        body.insert(data_field.to_string(), json!({"text": stdin_text.trim()}));
        Ok(serde_json::Value::Object(body).to_string())
    }
}

#[cfg(test)]
mod parsing_tests {
    use super::*;
    use clap::{Arg, Command};

    fn make_matches(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("json").long("json"))
            .arg(Arg::new("text").long("text"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_sanitize_args_json() {
        let matches = make_matches(&["test", "--json", "{\"foo\":\"bar\"}"]);
        let body = parse_sanitize_args(&matches, "field").unwrap();
        assert_eq!(body, "{\"foo\":\"bar\"}");
    }

    #[test]
    fn test_parse_sanitize_args_text() {
        let matches = make_matches(&["test", "--text", "hello"]);
        let body = parse_sanitize_args(&matches, "field").unwrap();
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["field"]["text"], "hello");
    }

    #[test]
    fn test_build_create_template_url() {
        let config = CreateTemplateConfig {
            project: "p".to_string(),
            location: "us-central1".to_string(),
            template_id: "t".to_string(),
            body: "{}".to_string(),
        };
        let url = build_create_template_url(&config);
        // encode_path_segment encodes hyphens ('-' → '%2D')
        assert_eq!(
            url,
            "https://modelarmor.us-central1.rep.googleapis.com/v1/projects/p/locations/us%2Dcentral1/templates?templateId=t"
        );
    }

    fn make_matches_create(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("project").long("project").required(true))
            .arg(Arg::new("location").long("location").required(true))
            .arg(Arg::new("template-id").long("template-id").required(true))
            .arg(Arg::new("json").long("json"))
            .arg(Arg::new("preset").long("preset"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_create_template_args_json() {
        let matches = make_matches_create(&[
            "test",
            "--project",
            "p",
            "--location",
            "l",
            "--template-id",
            "t",
            "--json",
            "{\"a\":1}",
        ]);
        let config = parse_create_template_args(&matches).unwrap();
        assert_eq!(config.project, "p");
        assert_eq!(config.location, "l");
        assert_eq!(config.template_id, "t");
        assert_eq!(config.body, "{\"a\":1}");
    }

    #[test]
    fn test_parse_create_template_args_preset() {
        let matches = make_matches_create(&[
            "test",
            "--project",
            "p",
            "--location",
            "l",
            "--template-id",
            "t",
            "--preset",
            "jailbreak",
        ]);
        let config = parse_create_template_args(&matches).unwrap();
        assert_eq!(config.project, "p");
        assert_eq!(config.location, "l");
        assert_eq!(config.template_id, "t");
        assert!(config.body.contains("piAndJailbreakFilterSettings"));
    }

    #[test]
    fn test_load_preset_template_fallback() {
        // Will test loading the built-in preset template
        let content = load_preset_template("jailbreak").unwrap();
        assert!(content.contains("piAndJailbreakFilterSettings"));
    }

    #[test]
    fn test_inject_commands() {
        let helper = ModelArmorHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();

        let cmd = helper.inject_commands(cmd, &doc);
        let subcommands: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subcommands.contains(&"+sanitize-prompt"));
        assert!(subcommands.contains(&"+sanitize-response"));
        assert!(subcommands.contains(&"+create-template"));
    }

    #[test]
    fn test_build_create_template_url_encodes_segments() {
        let config = CreateTemplateConfig {
            project: "my-project".to_string(),
            location: "us-central1".to_string(),
            template_id: "my-template".to_string(),
            body: "{}".to_string(),
        };
        let url = build_create_template_url(&config);
        assert!(url.contains("projects/my%2Dproject"));
        assert!(url.contains("locations/us%2Dcentral1"));
        assert!(url.contains("templateId=my%2Dtemplate"));
    }

    #[test]
    fn test_parse_create_template_args_rejects_traversal() {
        let matches = make_matches_create(&[
            "test",
            "--project",
            "../etc",
            "--location",
            "us-central1",
            "--template-id",
            "t",
            "--preset",
            "jailbreak",
        ]);
        assert!(parse_create_template_args(&matches).is_err());
    }
}

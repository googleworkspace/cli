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

//! Model Armor sanitization types and helpers.
//!
//! Extracted from `helpers::modelarmor` so that library consumers can use
//! sanitization without pulling in CLI-only helper infrastructure.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::GwsError;

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

impl From<&str> for SanitizeMode {
    /// Parses a string into a `SanitizeMode`.
    ///
    /// * "block" (case-insensitive) -> `Block`
    /// * Any other value -> `Warn` (safe default)
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "block" => SanitizeMode::Block,
            _ => SanitizeMode::Warn,
        }
    }
}

pub const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Sanitize text through a Model Armor template and return the result.
/// Template format: projects/PROJECT/locations/LOCATION/templates/TEMPLATE
pub async fn sanitize_text(template: &str, text: &str) -> Result<SanitizationResult, GwsError> {
    let (body, url) = build_sanitize_request_data(template, text, "sanitizeUserPrompt")?;

    let token = crate::auth::get_token(&[CLOUD_PLATFORM_SCOPE])
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

/// Build the regional base URL for Model Armor API.
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

pub fn build_sanitize_request_data(
    template: &str,
    text: &str,
    method: &str,
) -> Result<(String, String), GwsError> {
    let template = crate::validate::validate_resource_name(template)?;
    let location = extract_location(template).ok_or_else(|| {
        GwsError::Validation(
            "Cannot extract location from --sanitize template. Expected format: projects/PROJECT/locations/LOCATION/templates/TEMPLATE".to_string(),
        )
    })?;
    let location = crate::validate::validate_api_identifier(location)?;

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

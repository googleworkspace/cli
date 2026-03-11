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

use serde_json::json;
use thiserror::Error;

pub(crate) fn is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

pub(crate) fn yellow(s: &str) -> String {
    if is_tty() {
        format!("\x1b[33m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub(crate) fn red(s: &str) -> String {
    if is_tty() {
        format!("\x1b[31m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub(crate) fn bold(s: &str) -> String {
    if is_tty() {
        format!("\x1b[1m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

#[derive(Error, Debug)]
pub enum GwsError {
    #[error("{message}")]
    Api {
        code: u16,
        message: String,
        reason: String,
        /// For `accessNotConfigured` errors: the GCP console URL to enable the API.
        enable_url: Option<String>,
    },

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    Auth(String),

    #[error("{0}")]
    Discovery(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl GwsError {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            GwsError::Api {
                code,
                message,
                reason,
                enable_url,
            } => {
                let mut error_obj = json!({
                    "code": code,
                    "message": message,
                    "reason": reason,
                });
                // Include enable_url in JSON output when present (accessNotConfigured errors).
                // This preserves machine-readable compatibility while adding new optional field.
                if let Some(url) = enable_url {
                    error_obj["enable_url"] = json!(url);
                }
                json!({ "error": error_obj })
            }
            GwsError::Validation(msg) => json!({
                "error": {
                    "code": 400,
                    "message": msg,
                    "reason": "validationError",
                }
            }),
            GwsError::Auth(msg) => json!({
                "error": {
                    "code": 401,
                    "message": msg,
                    "reason": "authError",
                }
            }),
            GwsError::Discovery(msg) => json!({
                "error": {
                    "code": 500,
                    "message": msg,
                    "reason": "discoveryError",
                }
            }),
            GwsError::Other(e) => json!({
                "error": {
                    "code": 500,
                    "message": format!("{e:#}"),
                    "reason": "internalError",
                }
            }),
        }
    }
}

/// Formats any error as a JSON object and prints to stdout.
///
/// For `accessNotConfigured` errors (HTTP 403, reason `accessNotConfigured`),
/// additional human-readable guidance is printed to stderr explaining how to
/// enable the API in GCP. The JSON output on stdout is unchanged (machine-readable).
pub fn print_error_json(err: &GwsError) {
    let json = err.to_json();
    println!(
        "{}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );

    // Print actionable guidance to stderr for accessNotConfigured errors
    if let GwsError::Api {
        reason, enable_url, ..
    } = err
    {
        if reason == "accessNotConfigured" {
            eprintln!();
            eprintln!("{}", yellow("💡 API not enabled for your GCP project."));
            if let Some(url) = enable_url {
                eprintln!("   Enable it at: {url}");
            } else {
                eprintln!("   Visit the GCP Console → APIs & Services → Library to enable the required API.");
            }
            eprintln!("   After enabling, wait a few seconds and retry your command.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // In test environments stderr is not a TTY, so the helpers return plain strings.
    #[test]
    fn test_yellow_non_tty() {
        // Stderr is not a TTY in CI/test runners; plain string is returned.
        let result = yellow("hello");
        // Either plain or ANSI-wrapped depending on environment; must contain the text.
        assert!(result.contains("hello"));
    }

    #[test]
    fn test_red_non_tty() {
        let result = red("error");
        assert!(result.contains("error"));
    }

    #[test]
    fn test_bold_non_tty() {
        let result = bold("important");
        assert!(result.contains("important"));
    }

    #[test]
    fn test_color_helpers_no_ansi_when_non_tty() {
        // In test runners, stderr is not a TTY — helpers must return the raw string.
        if !is_tty() {
            assert_eq!(yellow("x"), "x");
            assert_eq!(red("x"), "x");
            assert_eq!(bold("x"), "x");
        }
    }

    #[test]
    fn test_error_to_json_api() {
        let err = GwsError::Api {
            code: 404,
            message: "Not Found".to_string(),
            reason: "notFound".to_string(),
            enable_url: None,
        };
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 404);
        assert_eq!(json["error"]["message"], "Not Found");
        assert_eq!(json["error"]["reason"], "notFound");
        assert!(json["error"]["enable_url"].is_null());
    }

    #[test]
    fn test_error_to_json_validation() {
        let err = GwsError::Validation("Invalid input".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 400);
        assert_eq!(json["error"]["message"], "Invalid input");
        assert_eq!(json["error"]["reason"], "validationError");
    }

    #[test]
    fn test_error_to_json_auth() {
        let err = GwsError::Auth("Token expired".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 401);
        assert_eq!(json["error"]["message"], "Token expired");
        assert_eq!(json["error"]["reason"], "authError");
    }

    #[test]
    fn test_error_to_json_discovery() {
        let err = GwsError::Discovery("Failed to fetch doc".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 500);
        assert_eq!(json["error"]["message"], "Failed to fetch doc");
        assert_eq!(json["error"]["reason"], "discoveryError");
    }

    #[test]
    fn test_error_to_json_other() {
        let err = GwsError::Other(anyhow::anyhow!("Something went wrong"));
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 500);
        assert_eq!(json["error"]["message"], "Something went wrong");
        assert_eq!(json["error"]["reason"], "internalError");
    }

    // --- accessNotConfigured tests ---

    #[test]
    fn test_error_to_json_access_not_configured_with_url() {
        let err = GwsError::Api {
            code: 403,
            message: "Gmail API has not been used in project 549352339482 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482 then retry.".to_string(),
            reason: "accessNotConfigured".to_string(),
            enable_url: Some("https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482".to_string()),
        };
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 403);
        assert_eq!(json["error"]["reason"], "accessNotConfigured");
        assert_eq!(
            json["error"]["enable_url"],
            "https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482"
        );
    }

    #[test]
    fn test_error_to_json_access_not_configured_without_url() {
        let err = GwsError::Api {
            code: 403,
            message: "API not enabled.".to_string(),
            reason: "accessNotConfigured".to_string(),
            enable_url: None,
        };
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 403);
        assert_eq!(json["error"]["reason"], "accessNotConfigured");
        // enable_url key should not appear in JSON when None
        assert!(json["error"]["enable_url"].is_null());
    }
}

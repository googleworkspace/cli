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

/// Human-readable exit code table, keyed by (code, description).
///
/// Used by `print_usage()` so the help text stays in sync with the
/// constants defined below without requiring manual updates in two places.
pub const EXIT_CODE_DOCUMENTATION: &[(i32, &str)] = &[
    (0, "Success"),
    (
        GwsError::EXIT_CODE_API,
        "API error  — Google returned an error response",
    ),
    (
        GwsError::EXIT_CODE_AUTH,
        "Auth error — credentials missing or invalid",
    ),
    (
        GwsError::EXIT_CODE_VALIDATION,
        "Validation — bad arguments or input",
    ),
    (
        GwsError::EXIT_CODE_DISCOVERY,
        "Discovery  — could not fetch API schema",
    ),
    (GwsError::EXIT_CODE_OTHER, "Internal   — unexpected failure"),
];

impl GwsError {
    /// Exit code for [`GwsError::Api`] variants.
    pub const EXIT_CODE_API: i32 = 1;
    /// Exit code for [`GwsError::Auth`] variants.
    pub const EXIT_CODE_AUTH: i32 = 2;
    /// Exit code for [`GwsError::Validation`] variants.
    pub const EXIT_CODE_VALIDATION: i32 = 3;
    /// Exit code for [`GwsError::Discovery`] variants.
    pub const EXIT_CODE_DISCOVERY: i32 = 4;
    /// Exit code for [`GwsError::Other`] variants.
    pub const EXIT_CODE_OTHER: i32 = 5;

    /// Map each error variant to a stable, documented exit code.
    ///
    /// | Code | Meaning                                      |
    /// |------|----------------------------------------------|
    /// |  0   | Success (never returned here)                |
    /// |  1   | API error — Google returned an error response |
    /// |  2   | Auth error — credentials missing or invalid  |
    /// |  3   | Validation error — bad arguments or input    |
    /// |  4   | Discovery error — could not fetch API schema |
    /// |  5   | Internal error — unexpected failure          |
    pub fn exit_code(&self) -> i32 {
        match self {
            GwsError::Api { .. } => Self::EXIT_CODE_API,
            GwsError::Auth(_) => Self::EXIT_CODE_AUTH,
            GwsError::Validation(_) => Self::EXIT_CODE_VALIDATION,
            GwsError::Discovery(_) => Self::EXIT_CODE_DISCOVERY,
            GwsError::Other(_) => Self::EXIT_CODE_OTHER,
        }
    }

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

/// Returns true when stderr is connected to an interactive terminal,
/// meaning ANSI color codes will be visible to the user.
fn stderr_supports_color() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Wrap `text` in ANSI bold + the given color code, resetting afterwards.
/// Returns the plain text unchanged when stderr is not a TTY.
fn colorize(text: &str, ansi_color: &str) -> String {
    if stderr_supports_color() {
        format!("\x1b[1;{ansi_color}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Format a colored error label for the given error variant.
fn error_label(err: &GwsError) -> String {
    match err {
        GwsError::Api { .. } => colorize("error[api]:", "31"),       // red
        GwsError::Auth(_) => colorize("error[auth]:", "31"),         // red
        GwsError::Validation(_) => colorize("error[validation]:", "33"), // yellow
        GwsError::Discovery(_) => colorize("error[discovery]:", "31"), // red
        GwsError::Other(_) => colorize("error:", "31"),              // red
    }
}

/// Formats any error as a JSON object and prints to stdout.
///
/// A human-readable colored label is printed to stderr when connected to a
/// TTY. For `accessNotConfigured` errors (HTTP 403, reason
/// `accessNotConfigured`), additional guidance is printed to stderr.
/// The JSON output on stdout is unchanged (machine-readable).
pub fn print_error_json(err: &GwsError) {
    let json = err.to_json();
    println!(
        "{}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );

    // Print a colored summary line to stderr so humans can quickly scan
    // the error type without parsing JSON.
    eprintln!("{} {}", error_label(err), err);

    // Print actionable guidance to stderr for accessNotConfigured errors
    if let GwsError::Api {
        reason, enable_url, ..
    } = err
    {
        if reason == "accessNotConfigured" {
            eprintln!();
            let hint = colorize("hint:", "36"); // cyan
            eprintln!("{hint} API not enabled for your GCP project.");
            if let Some(url) = enable_url {
                eprintln!("      Enable it at: {url}");
            } else {
                eprintln!("      Visit the GCP Console → APIs & Services → Library to enable the required API.");
            }
            eprintln!("      After enabling, wait a few seconds and retry your command.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_code_api() {
        let err = GwsError::Api {
            code: 404,
            message: "Not Found".to_string(),
            reason: "notFound".to_string(),
            enable_url: None,
        };
        assert_eq!(err.exit_code(), GwsError::EXIT_CODE_API);
    }

    #[test]
    fn test_exit_code_auth() {
        assert_eq!(
            GwsError::Auth("bad token".to_string()).exit_code(),
            GwsError::EXIT_CODE_AUTH
        );
    }

    #[test]
    fn test_exit_code_validation() {
        assert_eq!(
            GwsError::Validation("missing arg".to_string()).exit_code(),
            GwsError::EXIT_CODE_VALIDATION
        );
    }

    #[test]
    fn test_exit_code_discovery() {
        assert_eq!(
            GwsError::Discovery("fetch failed".to_string()).exit_code(),
            GwsError::EXIT_CODE_DISCOVERY
        );
    }

    #[test]
    fn test_exit_code_other() {
        assert_eq!(
            GwsError::Other(anyhow::anyhow!("oops")).exit_code(),
            GwsError::EXIT_CODE_OTHER
        );
    }

    #[test]
    fn test_exit_codes_are_distinct() {
        // Ensure all named constants are unique (regression guard).
        let codes = [
            GwsError::EXIT_CODE_API,
            GwsError::EXIT_CODE_AUTH,
            GwsError::EXIT_CODE_VALIDATION,
            GwsError::EXIT_CODE_DISCOVERY,
            GwsError::EXIT_CODE_OTHER,
        ];
        let unique: std::collections::HashSet<i32> = codes.iter().copied().collect();
        assert_eq!(
            unique.len(),
            codes.len(),
            "exit codes must be distinct: {codes:?}"
        );
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

    // --- colored output tests ---

    #[test]
    #[serial_test::serial]
    fn test_colorize_respects_no_color_env() {
        // NO_COLOR is the de-facto standard for disabling colors.
        // When set, colorize() should return the plain text.
        // Uses #[serial] because it modifies a global env var.
        std::env::set_var("NO_COLOR", "1");
        let result = colorize("hello", "31");
        std::env::remove_var("NO_COLOR");
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_error_label_contains_variant_name() {
        let api_err = GwsError::Api {
            code: 400,
            message: "bad".to_string(),
            reason: "r".to_string(),
            enable_url: None,
        };
        let label = error_label(&api_err);
        assert!(label.contains("error[api]:"));

        let auth_err = GwsError::Auth("fail".to_string());
        assert!(error_label(&auth_err).contains("error[auth]:"));

        let val_err = GwsError::Validation("bad input".to_string());
        assert!(error_label(&val_err).contains("error[validation]:"));

        let disc_err = GwsError::Discovery("missing".to_string());
        assert!(error_label(&disc_err).contains("error[discovery]:"));

        let other_err = GwsError::Other(anyhow::anyhow!("oops"));
        assert!(error_label(&other_err).contains("error:"));
    }
}

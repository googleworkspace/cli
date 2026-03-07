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

impl GwsError {
    /// Returns a semantic exit code for this error.
    ///
    /// Exit codes follow CLI best practices (loosely based on sysexits.h):
    /// - 0: success
    /// - 1: general failure
    /// - 2: usage/validation error (bad arguments)
    /// - 3: resource not found
    /// - 4: permission denied / auth failure
    /// - 5: conflict (resource already exists)
    /// - 75: temporary failure (network timeout, rate limit — retry may help)
    /// - 78: configuration error
    pub fn exit_code(&self) -> i32 {
        match self {
            GwsError::Validation(_) => 2,
            GwsError::Auth(_) => 4,
            GwsError::Discovery(_) => 78,
            GwsError::Api { code, .. } => match *code {
                404 => 3,
                401 | 403 => 4,
                409 => 5,
                408 | 429 | 500 | 502 | 503 | 504 => 75,
                _ => 1,
            },
            GwsError::Other(_) => 1,
        }
    }

    /// Returns true if this error is transient and retrying may succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            GwsError::Api {
                code: 408 | 429 | 500 | 502 | 503 | 504,
                ..
            }
        )
    }

    /// Returns an actionable fix suggestion for this error, if available.
    pub fn fix_hint(&self) -> Option<String> {
        match self {
            GwsError::Auth(_) => Some(
                "Run `gws auth login` to authenticate, or set GOOGLE_WORKSPACE_CLI_TOKEN."
                    .to_string(),
            ),
            GwsError::Discovery(_) => Some(
                "Check the service name with `gws --help`, or verify network connectivity."
                    .to_string(),
            ),
            GwsError::Validation(msg) => {
                if msg.contains("Required") && msg.contains("parameter") {
                    Some("Provide the missing parameter via --params '{\"key\": \"value\"}'.".to_string())
                } else if msg.contains("No service specified") || msg.contains("No resource") {
                    Some("Run `gws --help` to see available services and usage.".to_string())
                } else {
                    None
                }
            }
            GwsError::Api {
                code,
                reason,
                enable_url,
                ..
            } => {
                if reason == "accessNotConfigured" {
                    if let Some(url) = enable_url {
                        return Some(format!("Enable the API at: {url}"));
                    }
                    return Some("Enable the required API in GCP Console > APIs & Services > Library.".to_string());
                }
                match *code {
                    401 => Some(
                        "Run `gws auth login` to refresh your credentials.".to_string(),
                    ),
                    403 => Some(
                        "Check that your account has permission for this operation.".to_string(),
                    ),
                    404 => Some(
                        "Verify the resource ID. Use `gws schema <service>.<resource>.<method>` to inspect parameters.".to_string(),
                    ),
                    429 => Some(
                        "Rate limited — wait a moment and retry.".to_string(),
                    ),
                    _ => None,
                }
            }
            GwsError::Other(_) => None,
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
                    "transient": self.is_transient(),
                });
                // Include enable_url in JSON output when present (accessNotConfigured errors).
                // This preserves machine-readable compatibility while adding new optional field.
                if let Some(url) = enable_url {
                    error_obj["enable_url"] = json!(url);
                }
                if let Some(fix) = self.fix_hint() {
                    error_obj["fix"] = json!(fix);
                }
                json!({ "error": error_obj })
            }
            GwsError::Validation(msg) => {
                let mut error_obj = json!({
                    "code": 400,
                    "message": msg,
                    "reason": "validationError",
                    "transient": false,
                });
                if let Some(fix) = self.fix_hint() {
                    error_obj["fix"] = json!(fix);
                }
                json!({ "error": error_obj })
            }
            GwsError::Auth(msg) => json!({
                "error": {
                    "code": 401,
                    "message": msg,
                    "reason": "authError",
                    "transient": false,
                    "fix": self.fix_hint(),
                }
            }),
            GwsError::Discovery(msg) => json!({
                "error": {
                    "code": 500,
                    "message": msg,
                    "reason": "discoveryError",
                    "transient": false,
                    "fix": self.fix_hint(),
                }
            }),
            GwsError::Other(e) => json!({
                "error": {
                    "code": 500,
                    "message": format!("{e:#}"),
                    "reason": "internalError",
                    "transient": false,
                }
            }),
        }
    }
}

/// Formats any error as a JSON object and prints to stdout.
///
/// Human-readable guidance (fix hints) is printed to stderr so that stdout
/// remains machine-parseable. The JSON output on stdout includes `fix` and
/// `transient` fields for agent consumption.
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
            eprintln!("API not enabled for your GCP project.");
            if let Some(url) = enable_url {
                eprintln!("   Enable it at: {url}");
            } else {
                eprintln!("   Visit the GCP Console > APIs & Services > Library to enable the required API.");
            }
            eprintln!("   After enabling, wait a few seconds and retry your command.");
            return;
        }
    }

    // Print fix hint to stderr for other error types
    if let Some(fix) = err.fix_hint() {
        eprintln!();
        eprintln!("Fix: {fix}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(json["error"]["transient"], false);
        assert!(json["error"]["fix"].is_string(), "404 should include a fix hint");
    }

    #[test]
    fn test_error_to_json_validation() {
        let err = GwsError::Validation("Invalid input".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 400);
        assert_eq!(json["error"]["message"], "Invalid input");
        assert_eq!(json["error"]["reason"], "validationError");
        assert_eq!(json["error"]["transient"], false);
    }

    #[test]
    fn test_error_to_json_auth() {
        let err = GwsError::Auth("Token expired".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 401);
        assert_eq!(json["error"]["message"], "Token expired");
        assert_eq!(json["error"]["reason"], "authError");
        assert_eq!(json["error"]["transient"], false);
        assert!(json["error"]["fix"].is_string(), "auth errors should include a fix hint");
    }

    #[test]
    fn test_error_to_json_discovery() {
        let err = GwsError::Discovery("Failed to fetch doc".to_string());
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 500);
        assert_eq!(json["error"]["message"], "Failed to fetch doc");
        assert_eq!(json["error"]["reason"], "discoveryError");
        assert_eq!(json["error"]["transient"], false);
        assert!(json["error"]["fix"].is_string(), "discovery errors should include a fix hint");
    }

    #[test]
    fn test_error_to_json_other() {
        let err = GwsError::Other(anyhow::anyhow!("Something went wrong"));
        let json = err.to_json();
        assert_eq!(json["error"]["code"], 500);
        assert_eq!(json["error"]["message"], "Something went wrong");
        assert_eq!(json["error"]["reason"], "internalError");
        assert_eq!(json["error"]["transient"], false);
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
        assert!(json["error"]["fix"].is_string(), "accessNotConfigured should include fix with URL");
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
        assert!(json["error"]["fix"].is_string(), "accessNotConfigured should include fix even without URL");
    }

    // --- exit code tests ---

    #[test]
    fn test_exit_code_validation() {
        assert_eq!(GwsError::Validation("bad".to_string()).exit_code(), 2);
    }

    #[test]
    fn test_exit_code_auth() {
        assert_eq!(GwsError::Auth("denied".to_string()).exit_code(), 4);
    }

    #[test]
    fn test_exit_code_discovery() {
        assert_eq!(GwsError::Discovery("failed".to_string()).exit_code(), 78);
    }

    #[test]
    fn test_exit_code_api_not_found() {
        let err = GwsError::Api {
            code: 404,
            message: "Not Found".to_string(),
            reason: "notFound".to_string(),
            enable_url: None,
        };
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn test_exit_code_api_forbidden() {
        let err = GwsError::Api {
            code: 403,
            message: "Forbidden".to_string(),
            reason: "forbidden".to_string(),
            enable_url: None,
        };
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn test_exit_code_api_conflict() {
        let err = GwsError::Api {
            code: 409,
            message: "Conflict".to_string(),
            reason: "conflict".to_string(),
            enable_url: None,
        };
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn test_exit_code_api_rate_limit() {
        let err = GwsError::Api {
            code: 429,
            message: "Rate limited".to_string(),
            reason: "rateLimitExceeded".to_string(),
            enable_url: None,
        };
        assert_eq!(err.exit_code(), 75);
    }

    #[test]
    fn test_exit_code_api_server_error() {
        for code in [500, 502, 503, 504] {
            let err = GwsError::Api {
                code,
                message: "Server error".to_string(),
                reason: "backendError".to_string(),
                enable_url: None,
            };
            assert_eq!(err.exit_code(), 75, "HTTP {code} should be exit code 75");
        }
    }

    #[test]
    fn test_exit_code_other() {
        let err = GwsError::Other(anyhow::anyhow!("oops"));
        assert_eq!(err.exit_code(), 1);
    }

    // --- transient tests ---

    #[test]
    fn test_is_transient_true() {
        for code in [408, 429, 500, 502, 503, 504] {
            let err = GwsError::Api {
                code,
                message: "err".to_string(),
                reason: "err".to_string(),
                enable_url: None,
            };
            assert!(err.is_transient(), "HTTP {code} should be transient");
        }
    }

    #[test]
    fn test_is_transient_false() {
        for code in [400, 401, 403, 404, 409] {
            let err = GwsError::Api {
                code,
                message: "err".to_string(),
                reason: "err".to_string(),
                enable_url: None,
            };
            assert!(!err.is_transient(), "HTTP {code} should not be transient");
        }
        assert!(!GwsError::Validation("x".to_string()).is_transient());
        assert!(!GwsError::Auth("x".to_string()).is_transient());
        assert!(!GwsError::Discovery("x".to_string()).is_transient());
    }

    // --- fix hint tests ---

    #[test]
    fn test_fix_hint_auth() {
        let hint = GwsError::Auth("expired".to_string()).fix_hint();
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("gws auth login"));
    }

    #[test]
    fn test_fix_hint_discovery() {
        let hint = GwsError::Discovery("fail".to_string()).fix_hint();
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("gws --help"));
    }

    #[test]
    fn test_fix_hint_validation_missing_param() {
        let hint = GwsError::Validation("Required parameter 'fileId' is missing".to_string())
            .fix_hint();
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("--params"));
    }

    #[test]
    fn test_fix_hint_validation_no_service() {
        let hint = GwsError::Validation("No service specified".to_string()).fix_hint();
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("gws --help"));
    }

    #[test]
    fn test_fix_hint_validation_generic() {
        // Generic validation errors may not have a fix hint
        let hint = GwsError::Validation("something weird".to_string()).fix_hint();
        assert!(hint.is_none());
    }

    #[test]
    fn test_fix_hint_api_rate_limit() {
        let err = GwsError::Api {
            code: 429,
            message: "Rate limited".to_string(),
            reason: "rateLimitExceeded".to_string(),
            enable_url: None,
        };
        let hint = err.fix_hint();
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("retry"));
    }

    #[test]
    fn test_fix_hint_other_has_none() {
        assert!(GwsError::Other(anyhow::anyhow!("oops")).fix_hint().is_none());
    }
}

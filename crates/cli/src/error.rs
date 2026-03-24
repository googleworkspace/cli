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

//! Structured error types and CLI error output.
//!
//! Core error types are re-exported from the `google_workspace` library crate.
//! CLI-specific error formatting (colored terminal output) is defined here.

pub use google_workspace::error::*;

use crate::output::{colorize, sanitize_for_terminal};

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

/// Format a colored error label for the given error variant.
fn error_label(err: &GwsError) -> String {
    match err {
        GwsError::Api { .. } => colorize("error[api]:", "31"), // red
        GwsError::Auth(_) => colorize("error[auth]:", "31"),   // red
        GwsError::Validation(_) => colorize("error[validation]:", "33"), // yellow
        GwsError::Discovery(_) => colorize("error[discovery]:", "31"), // red
        GwsError::Other(_) => colorize("error:", "31"),        // red
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

    // Print a colored summary to stderr. For accessNotConfigured errors,
    // print specialized guidance instead of the generic message to avoid
    // redundant output (the full API error already appears in the JSON).
    if let GwsError::Api {
        reason, enable_url, ..
    } = err
    {
        if reason == "accessNotConfigured" {
            eprintln!();
            let hint = colorize("hint:", "36"); // cyan
            eprintln!(
                "{} {hint} API not enabled for your GCP project.",
                error_label(err)
            );
            if let Some(url) = enable_url {
                eprintln!("      Enable it at: {url}");
            } else {
                eprintln!("      Visit the GCP Console → APIs & Services → Library to enable the required API.");
            }
            eprintln!("      After enabling, wait a few seconds and retry your command.");
            return;
        }
    }
    eprintln!(
        "{} {}",
        error_label(err),
        sanitize_for_terminal(&err.to_string())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn test_colorize_respects_no_color_env() {
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

    #[test]
    fn test_sanitize_for_terminal_strips_control_chars() {
        let input = "normal \x1b[31mred text\x1b[0m end";
        let sanitized = sanitize_for_terminal(input);
        assert_eq!(sanitized, "normal [31mred text[0m end");
        assert!(!sanitized.contains('\x1b'));

        let input2 = "line1\nline2\ttab";
        assert_eq!(sanitize_for_terminal(input2), "line1\nline2\ttab");

        let input3 = "hello\x07bell\x08backspace";
        assert_eq!(sanitize_for_terminal(input3), "hellobellbackspace");
    }
}

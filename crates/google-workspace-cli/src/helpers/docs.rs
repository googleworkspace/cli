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
use crate::executor;
use clap::{Arg, ArgMatches, Command};
use anyhow::anyhow;
use serde_json::json;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub struct DocsHelper;

impl Helper for DocsHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+write")
                .about("[Helper] Append text to a document")
                .arg(
                    Arg::new("document")
                        .long("document")
                        .help("Document ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("text")
                        .long("text")
                        .help("Text to append (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws docs +write --document DOC_ID --text 'Hello, world!'

TIPS:
  Text is inserted at the end of the document body.
  For rich formatting, use the raw batchUpdate API instead.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+suggest")
                .about("[Helper] Suggest an edit in a document (uses Playwright)")
                .long_about(
                    "Create a tracked suggestion in a Google Doc by automating the browser UI.\n\n\
                     The Google Docs API has no support for Suggesting mode — all API writes are \
                     direct edits. This command works around that limitation by launching a headless \
                     browser via Playwright, switching to Suggesting mode, and performing a \
                     Find & Replace so the change appears as a suggestion that collaborators can \
                     accept or reject.\n\n\
                     PREREQUISITES:\n  \
                     - Node.js 18+ and Playwright: npx playwright install chromium\n  \
                     - A saved browser session: npx playwright codegen --save-storage=state.json docs.google.com\n    \
                       (log in, then close the browser to save the state file)",
                )
                .arg(
                    Arg::new("document")
                        .long("document")
                        .help("Document ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("find")
                        .long("find")
                        .help("Exact text to find (must match exactly once)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("replace")
                        .long("replace")
                        .help("Replacement text (recorded as a suggestion)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("state-file")
                        .long("state-file")
                        .help("Path to Playwright browser state JSON")
                        .value_name("PATH")
                        .default_value("~/.config/gws/playwright-state.json"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws docs +suggest --document DOC_ID --find 'old text' --replace 'new text'

WHY:
  The Google Docs API v1 has no method to create suggestions. This command
  automates the browser UI to work around that decade-old limitation.
  See: https://issuetracker.google.com/issues/36054544",
                ),
        );
        cmd
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(matches) = matches.subcommand_matches("+write") {
                let (params_str, body_str, scopes) = build_write_request(matches, doc)?;

                let scope_strs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scope_strs).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) if matches.get_flag("dry-run") => (None, executor::AuthMethod::None),
                    Err(e) => return Err(GwsError::Auth(format!("Docs auth failed: {e}"))),
                };

                // Method: documents.batchUpdate
                let documents_res = doc.resources.get("documents").ok_or_else(|| {
                    GwsError::Discovery("Resource 'documents' not found".to_string())
                })?;
                let batch_update_method =
                    documents_res.methods.get("batchUpdate").ok_or_else(|| {
                        GwsError::Discovery("Method 'documents.batchUpdate' not found".to_string())
                    })?;

                let pagination = executor::PaginationConfig {
                    page_all: false,
                    page_limit: 10,
                    page_delay_ms: 100,
                };

                executor::execute_method(
                    doc,
                    batch_update_method,
                    Some(&params_str),
                    Some(&body_str),
                    token.as_deref(),
                    auth_method,
                    None,
                    None,
                    matches.get_flag("dry-run"),
                    &pagination,
                    None,
                    &crate::helpers::modelarmor::SanitizeMode::Warn,
                    &crate::formatter::OutputFormat::default(),
                    false,
                )
                .await?;

                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+suggest") {
                return run_suggest(matches).await.map(|_| true);
            }

            Ok(false)
        })
    }
}

fn build_write_request(
    matches: &ArgMatches,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let document_id = matches.get_one::<String>("document").unwrap();
    let text = matches.get_one::<String>("text").unwrap();

    let documents_res = doc
        .resources
        .get("documents")
        .ok_or_else(|| GwsError::Discovery("Resource 'documents' not found".to_string()))?;
    let batch_update_method = documents_res.methods.get("batchUpdate").ok_or_else(|| {
        GwsError::Discovery("Method 'documents.batchUpdate' not found".to_string())
    })?;

    let params = json!({
        "documentId": document_id
    });

    let body = json!({
        "requests": [
            {
                "insertText": {
                    "text": text,
                    "endOfSegmentLocation": {
                        "segmentId": "" // Empty means body
                    }
                }
            }
        ]
    });

    let scopes: Vec<String> = batch_update_method
        .scopes
        .iter()
        .map(|s| s.to_string())
        .collect();

    Ok((params.to_string(), body.to_string(), scopes))
}

/// Run the Playwright-based suggest script as a subprocess.
///
/// The Google Docs API has no Suggesting mode support (see
/// https://issuetracker.google.com/issues/36054544). This function shells out
/// to a bundled Node.js script that automates the Docs UI via Playwright to
/// create tracked suggestions.
async fn run_suggest(matches: &ArgMatches) -> Result<(), GwsError> {
    let document = matches.get_one::<String>("document").unwrap();
    let find = matches.get_one::<String>("find").unwrap();
    let replace = matches.get_one::<String>("replace").unwrap();
    let state_file_raw = matches.get_one::<String>("state-file").unwrap();

    // Expand ~ to home directory
    let state_file = if state_file_raw.starts_with("~/") {
        dirs::home_dir()
            .map(|h| h.join(&state_file_raw[2..]))
            .unwrap_or_else(|| PathBuf::from(state_file_raw))
    } else {
        PathBuf::from(state_file_raw)
    };

    if !state_file.exists() {
        return Err(GwsError::Validation(format!(
            "Browser state file not found: {}\n\
             \n\
             To create one, run:\n  \
             npx playwright install chromium\n  \
             npx playwright codegen --save-storage=state.json docs.google.com\n\
             \n\
             Log in to your Google account in the browser that opens, then close it.\n\
             Move the saved state to: {}",
            state_file.display(),
            state_file.display(),
        )));
    }

    // Embed the Playwright script in the binary so it works regardless of
    // install method (cargo install, pre-built binary, npm, etc.)
    static SCRIPT: &str = include_str!("../../../../scripts/playwright-suggest.mjs");

    let script_file = tempfile::Builder::new()
        .suffix(".mjs")
        .tempfile()
        .map_err(|e| GwsError::Other(anyhow!("Failed to create temp file for script: {e}")))?;
    std::fs::write(script_file.path(), SCRIPT)
        .map_err(|e| GwsError::Other(anyhow!("Failed to write script to temp file: {e}")))?;

    let output = tokio::process::Command::new("node")
        .arg(script_file.path())
        .arg("suggest")
        .arg(document)
        .arg(find)
        .arg(replace)
        .arg(&state_file)
        .output()
        .await
        .map_err(|e| {
            GwsError::Other(anyhow!(
                "Failed to launch Playwright script (is Node.js installed?): {e}"
            ))
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let detail = if !stderr.is_empty() {
            stderr.to_string()
        } else {
            stdout.to_string()
        };
        return Err(GwsError::Other(anyhow!(
            "Playwright script failed (exit {}):\n{detail}",
            output.status.code().unwrap_or(-1)
        )));
    }

    // Parse the JSON output — treat unparseable output as an error
    let result: serde_json::Value =
        serde_json::from_str(stdout.trim()).map_err(|e| {
            GwsError::Other(anyhow!(
                "Playwright script returned invalid JSON ({e}):\n{stdout}"
            ))
        })?;

    if result.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        let error = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        return Err(GwsError::Other(anyhow!(error.to_string())));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{RestDescription, RestMethod, RestResource};
    use std::collections::HashMap;

    fn make_mock_doc() -> RestDescription {
        let mut methods = HashMap::new();
        methods.insert(
            "batchUpdate".to_string(),
            RestMethod {
                scopes: vec!["https://scope".to_string()],
                ..Default::default()
            },
        );

        let mut documents_res = RestResource::default();
        documents_res.methods = methods;

        let mut resources = HashMap::new();
        resources.insert("documents".to_string(), documents_res);

        RestDescription {
            resources,
            ..Default::default()
        }
    }

    fn make_matches_write(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("document").long("document"))
            .arg(Arg::new("text").long("text"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_build_write_request() {
        let doc = make_mock_doc();
        let matches = make_matches_write(&["test", "--document", "123", "--text", "hello world"]);
        let (params, body, scopes) = build_write_request(&matches, &doc).unwrap();

        assert!(params.contains("123"));
        assert!(body.contains("hello world"));
        assert!(body.contains("endOfSegmentLocation"));
        assert_eq!(scopes[0], "https://scope");
    }
}

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
use crate::error::{self, GwsError};
use crate::executor;
use clap::{Arg, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

/// MIME types for Google Workspace editor files where the Drive API `anchor`
/// field on comments is saved but silently ignored by the editor UI.
/// See: https://developers.google.com/workspace/drive/api/v3/manage-comments
const WORKSPACE_EDITOR_MIMES: &[&str] = &[
    "application/vnd.google-apps.document",
    "application/vnd.google-apps.spreadsheet",
    "application/vnd.google-apps.presentation",
    "application/vnd.google-apps.drawing",
];

pub struct DriveHelper;

impl Helper for DriveHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+upload")
                .about("[Helper] Upload a file with automatic metadata")
                .arg(
                    Arg::new("file")
                        .help("Path to file to upload")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("parent")
                        .long("parent")
                        .help("Parent folder ID")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("name")
                        .long("name")
                        .help("Target filename (defaults to source filename)")
                        .value_name("NAME"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws drive +upload ./report.pdf
  gws drive +upload ./report.pdf --parent FOLDER_ID
  gws drive +upload ./data.csv --name 'Sales Data.csv'

TIPS:
  MIME type is detected automatically.
  Filename is inferred from the local path unless --name is given.",
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
            // Check for anchored comments on Workspace editor files (#169)
            warn_anchored_comment_if_needed(doc, matches).await;

            if let Some(matches) = matches.subcommand_matches("+upload") {
                let file_path = matches.get_one::<String>("file").unwrap();
                let parent_id = matches.get_one::<String>("parent");
                let name_arg = matches.get_one::<String>("name");

                // Determine filename
                let filename = determine_filename(file_path, name_arg.map(|s| s.as_str()))?;

                // Find method: files.create
                let files_res = doc
                    .resources
                    .get("files")
                    .ok_or_else(|| GwsError::Discovery("Resource 'files' not found".to_string()))?;
                let create_method = files_res.methods.get("create").ok_or_else(|| {
                    GwsError::Discovery("Method 'files.create' not found".to_string())
                })?;

                // Build metadata
                let metadata = build_metadata(&filename, parent_id.map(|s| s.as_str()));

                let body_str = metadata.to_string();

                let scopes: Vec<&str> = create_method.scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scopes).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) => (None, executor::AuthMethod::None),
                };

                executor::execute_method(
                    doc,
                    create_method,
                    None,
                    Some(&body_str),
                    token.as_deref(),
                    auth_method,
                    None,
                    Some(file_path),
                    None,
                    matches.get_flag("dry-run"),
                    &executor::PaginationConfig::default(),
                    None,
                    &crate::helpers::modelarmor::SanitizeMode::Warn,
                    &crate::formatter::OutputFormat::default(),
                    false,
                )
                .await?;

                return Ok(true);
            }
            Ok(false)
        })
    }
}

/// Warn when `comments create` includes an `anchor` field targeting a Workspace
/// editor file (Docs, Sheets, Slides, Drawings). The Drive API accepts the anchor
/// but the editor UI silently ignores it, showing "Original content deleted".
///
/// This does not block execution — the dynamic dispatcher still runs the request.
async fn warn_anchored_comment_if_needed(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) {
    // Walk the dynamic subcommand tree: comments → create
    let create_matches = matches
        .subcommand_matches("comments")
        .and_then(|m| m.subcommand_matches("create"));

    let create_matches = match create_matches {
        Some(m) => m,
        None => return,
    };

    // Check if the --json body contains an "anchor" field
    let body_str = match create_matches.try_get_one::<String>("json").ok().flatten() {
        Some(s) => s.as_str(),
        None => return,
    };

    let body: Value = match serde_json::from_str(body_str) {
        Ok(v) => v,
        Err(_) => return, // Invalid JSON will be caught later by the executor
    };

    if body.get("anchor").is_none() {
        return;
    }

    // Extract fileId from --params
    let file_id = create_matches
        .get_one::<String>("params")
        .and_then(|p| serde_json::from_str::<Value>(p).ok())
        .and_then(|v| v.get("fileId").and_then(|id| id.as_str()).map(String::from));

    let file_id = match file_id {
        Some(id) => id,
        None => return, // Missing fileId will be caught later by the executor
    };

    // Try to fetch the file's mimeType to give a precise warning.
    // If auth or the request fails, fall back to a general warning.
    match fetch_file_mime_type(doc, &file_id).await {
        Ok(mime) if is_workspace_editor_mime(&mime) => {
            eprintln!(
                "\n{}",
                error::yellow(&format!(
                    "⚠️  Warning: anchor field ignored for {} files.\n   \
                     Google Workspace editors treat anchored comments as un-anchored.\n   \
                     The comment will be created but may show as \"Original content deleted\".\n   \
                     See: https://developers.google.com/workspace/drive/api/v3/manage-comments\n",
                    mime_display_name(&mime),
                )),
            );
        }
        Ok(_) => {} // Non-editor file — anchor should work fine
        Err(_) => {
            // Could not determine file type; print a general warning
            eprintln!(
                "\n{}",
                error::yellow(
                    "⚠️  Warning: anchor field may be ignored for Google Workspace editor files \
                     (Docs, Sheets, Slides).\n   \
                     If the target is a Workspace file, the comment may show as \
                     \"Original content deleted\".\n   \
                     See: https://developers.google.com/workspace/drive/api/v3/manage-comments\n",
                ),
            );
        }
    }
}

/// Fetch the mimeType of a Drive file by ID.
async fn fetch_file_mime_type(
    doc: &crate::discovery::RestDescription,
    file_id: &str,
) -> Result<String, GwsError> {
    let files_res = doc
        .resources
        .get("files")
        .ok_or_else(|| GwsError::Discovery("Resource 'files' not found".to_string()))?;
    let get_method = files_res
        .methods
        .get("get")
        .ok_or_else(|| GwsError::Discovery("Method 'files.get' not found".to_string()))?;

    let scopes: Vec<&str> = get_method.scopes.iter().map(|s| s.as_str()).collect();
    let token = auth::get_token(&scopes).await?;

    let params = format!(r#"{{"fileId":"{}","fields":"mimeType"}}"#, file_id);
    let output = executor::execute_method(
        doc,
        get_method,
        Some(&params),
        None,
        Some(&token),
        executor::AuthMethod::OAuth,
        None,
        None,
        false,
        &executor::PaginationConfig::default(),
        None,
        &crate::helpers::modelarmor::SanitizeMode::Warn,
        &crate::formatter::OutputFormat::default(),
        true, // capture_output — don't print to stdout
    )
    .await?;

    // extract mimeType from captured output
    output
        .and_then(|v| v.get("mimeType").and_then(|m| m.as_str()).map(String::from))
        .ok_or_else(|| GwsError::Other(anyhow::anyhow!("mimeType not found in response")))
}

fn is_workspace_editor_mime(mime: &str) -> bool {
    WORKSPACE_EDITOR_MIMES.contains(&mime)
}

fn mime_display_name(mime: &str) -> &str {
    match mime {
        "application/vnd.google-apps.document" => "Google Docs",
        "application/vnd.google-apps.spreadsheet" => "Google Sheets",
        "application/vnd.google-apps.presentation" => "Google Slides",
        "application/vnd.google-apps.drawing" => "Google Drawings",
        _ => "Google Workspace",
    }
}

fn determine_filename(file_path: &str, name_arg: Option<&str>) -> Result<String, GwsError> {
    if let Some(n) = name_arg {
        Ok(n.to_string())
    } else {
        Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| GwsError::Validation("Invalid file path".to_string()))
    }
}

fn build_metadata(filename: &str, parent_id: Option<&str>) -> Value {
    let mut metadata = json!({
        "name": filename
    });

    if let Some(parent) = parent_id {
        metadata["parents"] = json!([parent]);
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_filename_explicit() {
        assert_eq!(
            determine_filename("path/to/file.txt", Some("custom.txt")).unwrap(),
            "custom.txt"
        );
    }

    #[test]
    fn test_determine_filename_from_path() {
        assert_eq!(
            determine_filename("path/to/file.txt", None).unwrap(),
            "file.txt"
        );
    }

    #[test]
    fn test_determine_filename_invalid_path() {
        assert!(determine_filename("", None).is_err());
        assert!(determine_filename("/", None).is_err()); // Root has no filename component usually
    }

    #[test]
    fn test_build_metadata_no_parent() {
        let meta = build_metadata("file.txt", None);
        assert_eq!(meta["name"], "file.txt");
        assert!(meta.get("parents").is_none());
    }

    #[test]
    fn test_build_metadata_with_parent() {
        let meta = build_metadata("file.txt", Some("folder123"));
        assert_eq!(meta["name"], "file.txt");
        assert_eq!(meta["parents"][0], "folder123");
    }

    #[test]
    fn test_is_workspace_editor_mime() {
        assert!(is_workspace_editor_mime(
            "application/vnd.google-apps.document"
        ));
        assert!(is_workspace_editor_mime(
            "application/vnd.google-apps.spreadsheet"
        ));
        assert!(is_workspace_editor_mime(
            "application/vnd.google-apps.presentation"
        ));
        assert!(is_workspace_editor_mime(
            "application/vnd.google-apps.drawing"
        ));
        assert!(!is_workspace_editor_mime("application/pdf"));
        assert!(!is_workspace_editor_mime("image/png"));
        assert!(!is_workspace_editor_mime("text/plain"));
    }

    #[test]
    fn test_mime_display_name() {
        assert_eq!(
            mime_display_name("application/vnd.google-apps.document"),
            "Google Docs"
        );
        assert_eq!(
            mime_display_name("application/vnd.google-apps.spreadsheet"),
            "Google Sheets"
        );
        assert_eq!(
            mime_display_name("application/vnd.google-apps.presentation"),
            "Google Slides"
        );
        assert_eq!(
            mime_display_name("application/vnd.google-apps.drawing"),
            "Google Drawings"
        );
        assert_eq!(mime_display_name("application/pdf"), "Google Workspace");
    }
}

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
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

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
                .arg(
                    Arg::new("convert")
                        .long("convert")
                        .help("Convert to Google Docs format (auto-enabled for .md files)")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws drive +upload ./report.pdf
  gws drive +upload ./report.pdf --parent FOLDER_ID
  gws drive +upload ./data.csv --name 'Sales Data.csv'
  gws drive +upload ./notes.md --convert

TIPS:
  MIME type is detected automatically from the file extension.
  Markdown (.md) files are auto-converted to Google Docs.
  Use --convert to force conversion for other text formats.",
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
            if let Some(matches) = matches.subcommand_matches("+upload") {
                let file_path = matches.get_one::<String>("file").unwrap();
                let parent_id = matches.get_one::<String>("parent");
                let name_arg = matches.get_one::<String>("name");

                // Determine filename
                let filename = determine_filename(file_path, name_arg.map(|s| s.as_str()))?;

                // Auto-convert markdown files to Google Docs, or when --convert is set
                let is_markdown = infer_upload_mime(&filename) == Some("text/markdown");
                let convert = matches.get_flag("convert") || is_markdown;

                // Find method: files.create
                let files_res = doc
                    .resources
                    .get("files")
                    .ok_or_else(|| GwsError::Discovery("Resource 'files' not found".to_string()))?;
                let create_method = files_res.methods.get("create").ok_or_else(|| {
                    GwsError::Discovery("Method 'files.create' not found".to_string())
                })?;

                // Build metadata — when converting, sets target mimeType to Google Docs
                let metadata = build_metadata(&filename, parent_id.map(|s| s.as_str()), convert);

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

/// Infer the upload MIME type from the file extension.
/// Returns None for unknown extensions (the API will auto-detect).
fn infer_upload_mime(filename: &str) -> Option<&'static str> {
    match Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("md" | "markdown") => Some("text/markdown"),
        Some("csv") => Some("text/csv"),
        Some("html" | "htm") => Some("text/html"),
        Some("txt") => Some("text/plain"),
        Some("json") => Some("application/json"),
        _ => None,
    }
}

fn build_metadata(filename: &str, parent_id: Option<&str>, convert: bool) -> Value {
    let mut metadata = json!({
        "name": filename
    });

    // When converting, set the target Google Docs MIME type so the Drive API
    // converts the upload (e.g., markdown → Google Docs).
    if convert {
        metadata["mimeType"] = json!("application/vnd.google-apps.document");
    }

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
        let meta = build_metadata("file.txt", None, false);
        assert_eq!(meta["name"], "file.txt");
        assert!(meta.get("parents").is_none());
        assert!(meta.get("mimeType").is_none());
    }

    #[test]
    fn test_build_metadata_with_parent() {
        let meta = build_metadata("file.txt", Some("folder123"), false);
        assert_eq!(meta["name"], "file.txt");
        assert_eq!(meta["parents"][0], "folder123");
    }

    #[test]
    fn test_build_metadata_convert() {
        let meta = build_metadata("notes.md", None, true);
        assert_eq!(meta["name"], "notes.md");
        assert_eq!(meta["mimeType"], "application/vnd.google-apps.document");
    }

    #[test]
    fn test_infer_upload_mime_markdown() {
        assert_eq!(infer_upload_mime("notes.md"), Some("text/markdown"));
        assert_eq!(infer_upload_mime("README.markdown"), Some("text/markdown"));
        assert_eq!(infer_upload_mime("NOTES.MD"), Some("text/markdown"));
    }

    #[test]
    fn test_infer_upload_mime_other() {
        assert_eq!(infer_upload_mime("data.csv"), Some("text/csv"));
        assert_eq!(infer_upload_mime("page.html"), Some("text/html"));
        assert_eq!(infer_upload_mime("report.pdf"), None);
        assert_eq!(infer_upload_mime("image.png"), None);
    }
}

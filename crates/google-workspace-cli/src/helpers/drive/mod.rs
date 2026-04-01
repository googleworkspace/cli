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
pub mod download;
pub mod export;
pub mod move_file;
pub mod upload;

use download::handle_download;
use export::handle_export;
use move_file::handle_move;
use upload::handle_upload;

pub(super) use crate::auth;
pub(super) use crate::error::GwsError;
pub(super) use crate::executor;
pub(super) use anyhow::Context;
pub(super) use clap::{Arg, ArgMatches, Command};
pub(super) use google_workspace::validate::encode_path_segment;
pub(super) use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct DriveHelper;

pub(super) const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";
pub(super) const DRIVE_READONLY_SCOPE: &str =
    "https://www.googleapis.com/auth/drive.readonly";

/// MIME type prefix for Google Workspace native documents (Docs, Sheets, Slides, etc.).
/// Files with this prefix cannot be downloaded directly -- they must be exported.
pub(super) const GOOGLE_APPS_MIME_PREFIX: &str = "application/vnd.google-apps.";

impl Helper for DriveHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd
            .subcommand(
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
            )
            .subcommand(
                Command::new("+download")
                    .about("[Helper] Download a file by ID")
                    .arg(
                        Arg::new("file-id")
                            .help("The Drive file ID to download")
                            .required(true)
                            .index(1),
                    )
                    .arg(
                        Arg::new("output")
                            .long("output")
                            .short('o')
                            .help("Output file path (defaults to original filename in current directory)")
                            .value_name("PATH"),
                    )
                    .after_help(
                        "\
EXAMPLES:
  gws drive +download FILE_ID
  gws drive +download FILE_ID --output ./report.pdf

TIPS:
  Downloads the binary content of non-Google-Workspace files (PDFs, images, etc.).
  For Google Docs/Sheets/Slides, use +export instead.
  The original filename is fetched automatically from Drive metadata.",
                    ),
            )
            .subcommand(
                Command::new("+export")
                    .about("[Helper] Export a Google Workspace document to a local file")
                    .arg(
                        Arg::new("file-id")
                            .help("The Drive file ID to export")
                            .required(true)
                            .index(1),
                    )
                    .arg(
                        Arg::new("format")
                            .long("format")
                            .short('f')
                            .help("Export format (e.g., pdf, docx, xlsx, pptx, csv, tsv, txt, html, md, odt, ods, odp, rtf, epub, zip)")
                            .required(true)
                            .value_name("FORMAT"),
                    )
                    .arg(
                        Arg::new("output")
                            .long("output")
                            .short('o')
                            .help("Output file path (defaults to original filename with new extension)")
                            .value_name("PATH"),
                    )
                    .after_help(
                        "\
EXAMPLES:
  gws drive +export FILE_ID --format pdf
  gws drive +export FILE_ID --format docx --output ./report.docx
  gws drive +export FILE_ID -f xlsx -o ./data.xlsx

SUPPORTED FORMATS:
  Documents:  pdf, docx, odt, rtf, txt, html, epub, md, zip
  Spreadsheets: xlsx, ods, csv, tsv, pdf, zip
  Presentations: pptx, odp, pdf
  Drawings: png, jpg, svg, pdf

TIPS:
  Only works with Google Workspace files (Docs, Sheets, Slides).
  For regular files (PDFs, images), use +download instead.",
                    ),
            )
            .subcommand(
                Command::new("+move")
                    .about("[Helper] Move a file to a different folder")
                    .arg(
                        Arg::new("file-id")
                            .help("The Drive file ID to move")
                            .required(true)
                            .index(1),
                    )
                    .arg(
                        Arg::new("to")
                            .long("to")
                            .help("Destination folder ID")
                            .required(true)
                            .value_name("FOLDER_ID"),
                    )
                    .after_help(
                        "\
EXAMPLES:
  gws drive +move FILE_ID --to FOLDER_ID

TIPS:
  Moves the file from its current parent folder(s) to the destination folder.
  The file's current parent(s) are determined automatically.",
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
            if let Some(m) = matches.subcommand_matches("+upload") {
                handle_upload(doc, m).await?;
                return Ok(true);
            }
            if let Some(m) = matches.subcommand_matches("+download") {
                handle_download(m).await?;
                return Ok(true);
            }
            if let Some(m) = matches.subcommand_matches("+export") {
                handle_export(m).await?;
                return Ok(true);
            }
            if let Some(m) = matches.subcommand_matches("+move") {
                handle_move(m).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

// ---------------------------------------------------------------------------
// Shared utilities
// ---------------------------------------------------------------------------

/// Fetch file metadata from the Drive API.
///
/// The `fields` parameter controls which metadata fields are returned
/// (e.g., `"name,mimeType,size"` or `"id,name,parents"`).
pub(super) async fn fetch_file_metadata(
    client: &reqwest::Client,
    token: &str,
    encoded_id: &str,
    fields: &str,
) -> Result<Value, GwsError> {
    let meta_url = format!(
        "https://www.googleapis.com/drive/v3/files/{}",
        encoded_id,
    );
    let meta_resp = crate::client::send_with_retry(|| {
        client
            .get(&meta_url)
            .query(&[("fields", fields)])
            .bearer_auth(token)
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch file metadata: {e}")))?;

    if !meta_resp.status().is_success() {
        let status = meta_resp.status();
        let body = meta_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "metadata_fetch_failed".to_string(),
            enable_url: None,
        });
    }

    meta_resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse metadata: {e}")))
}

/// Stream a response body to a file, returning the total bytes written.
pub(super) async fn stream_to_file(
    response: reqwest::Response,
    path: &std::path::Path,
) -> Result<u64, GwsError> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut file = tokio::fs::File::create(path)
        .await
        .context("Failed to create output file")?;

    let mut stream = response.bytes_stream();
    let mut total_bytes: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read response chunk")?;
        file.write_all(&chunk)
            .await
            .context("Failed to write to file")?;
        total_bytes += chunk.len() as u64;
    }

    file.flush().await.context("Failed to flush file")?;
    Ok(total_bytes)
}

/// Resolve the output file path.
///
/// If `output` is provided, it is used as-is (or with the default name
/// appended if it ends with a path separator). Otherwise the `default_name`
/// (typically the remote filename) is used relative to the current directory.
///
/// The `default_name` is treated as untrusted (it comes from the Drive API),
/// so only the final path component (`.file_name()`) is used to prevent
/// directory traversal via crafted filenames like `../../evil` or `/etc/passwd`.
pub(super) fn resolve_output_path(
    output: Option<&str>,
    default_name: &str,
) -> Result<std::path::PathBuf, GwsError> {
    // Extract only the filename component from the untrusted remote name
    // to prevent path traversal (e.g., "../../.ssh/keys" -> ".ssh/keys" is
    // still dangerous, but file_name() yields just "keys").
    let safe_name = std::path::Path::new(default_name)
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("download"));

    match output {
        Some(p) => {
            let path = std::path::PathBuf::from(p);
            // If the output path looks like a directory (ends with separator), append the safe name
            if p.ends_with('/') || p.ends_with('\\') {
                Ok(path.join(safe_name))
            } else {
                Ok(path)
            }
        }
        None => Ok(std::path::PathBuf::from(safe_name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_output_path_none() {
        let path = resolve_output_path(None, "report.pdf").unwrap();
        assert_eq!(path, std::path::PathBuf::from("report.pdf"));
    }

    #[test]
    fn test_resolve_output_path_explicit() {
        let path = resolve_output_path(Some("my-file.pdf"), "report.pdf").unwrap();
        assert_eq!(path, std::path::PathBuf::from("my-file.pdf"));
    }

    #[test]
    fn test_resolve_output_path_dir_trailing_slash() {
        let path = resolve_output_path(Some("output/"), "report.pdf").unwrap();
        assert_eq!(path, std::path::PathBuf::from("output/report.pdf"));
    }

    #[test]
    fn test_resolve_output_path_strips_traversal_from_remote_name() {
        // Malicious remote filename with path traversal
        let path = resolve_output_path(None, "../../.ssh/authorized_keys").unwrap();
        assert_eq!(path, std::path::PathBuf::from("authorized_keys"));
    }

    #[test]
    fn test_resolve_output_path_strips_absolute_remote_name() {
        let path = resolve_output_path(None, "/etc/passwd").unwrap();
        assert_eq!(path, std::path::PathBuf::from("passwd"));
    }

    #[test]
    fn test_resolve_output_path_dir_with_traversal_remote_name() {
        let path = resolve_output_path(Some("output/"), "../../evil.txt").unwrap();
        assert_eq!(path, std::path::PathBuf::from("output/evil.txt"));
    }

    #[test]
    fn test_google_apps_mime_prefix_matches_workspace_types() {
        let workspace_types = [
            "application/vnd.google-apps.document",
            "application/vnd.google-apps.spreadsheet",
            "application/vnd.google-apps.presentation",
            "application/vnd.google-apps.drawing",
            "application/vnd.google-apps.form",
        ];
        for mime in workspace_types {
            assert!(
                mime.starts_with(GOOGLE_APPS_MIME_PREFIX),
                "{mime} should match Google Apps prefix"
            );
        }
    }

    #[test]
    fn test_google_apps_mime_prefix_rejects_regular_files() {
        let regular_types = [
            "application/pdf",
            "image/png",
            "text/plain",
            "application/octet-stream",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ];
        for mime in regular_types {
            assert!(
                !mime.starts_with(GOOGLE_APPS_MIME_PREFIX),
                "{mime} should not match Google Apps prefix"
            );
        }
    }
}

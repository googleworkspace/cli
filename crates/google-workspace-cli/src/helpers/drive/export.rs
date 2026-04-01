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

use super::*;
use std::path::Path;

/// Handle the `+export` subcommand.
pub(super) async fn handle_export(matches: &ArgMatches) -> Result<(), GwsError> {
    let file_id = matches.get_one::<String>("file-id").unwrap();
    let format = matches.get_one::<String>("format").unwrap();
    let output_path = matches.get_one::<String>("output");

    let dry_run = matches.get_flag("dry-run");

    let mime_type = format_to_mime(format)?;

    if dry_run {
        let info = json!({
            "dry_run": true,
            "action": "export",
            "file_id": file_id,
            "format": format,
            "mimeType": mime_type,
            "output": output_path,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&info).unwrap_or_default()
        );
        return Ok(());
    }

    let token = auth::get_token(&[DRIVE_READONLY_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Drive auth failed: {e}")))?;

    let client = crate::client::build_client()?;
    let encoded_id = encode_path_segment(file_id);

    // Step 1: Fetch metadata for the original filename and MIME type
    let metadata = fetch_file_metadata(&client, &token, &encoded_id, "name,mimeType").await?;

    let remote_name = metadata
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("export");

    // Guard: export only works with Google Workspace native documents
    if let Some(mime) = metadata.get("mimeType").and_then(|v| v.as_str()) {
        if !mime.starts_with(GOOGLE_APPS_MIME_PREFIX) {
            return Err(GwsError::Validation(format!(
                "File '{}' is not a Google Workspace document ({}). \
                 Use `gws drive +download` to download it directly.",
                crate::output::sanitize_for_terminal(remote_name),
                crate::output::sanitize_for_terminal(mime)
            )));
        }
    }

    // Build output filename with the export format extension.
    // Use file_stem from the remote name (untrusted, from Drive API) to
    // construct a safe filename like "MyDoc.pdf".
    let export_filename = {
        let safe_name = Path::new(remote_name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("export");
        let stem = Path::new(safe_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(safe_name);
        format!("{}.{}", stem, format)
    };

    let dest = match output_path {
        Some(p) if p.ends_with('/') || p.ends_with('\\') => {
            // Directory output: append the export filename with extension
            std::path::PathBuf::from(p).join(&export_filename)
        }
        Some(p) => {
            // Explicit file path: use as-is
            std::path::PathBuf::from(p)
        }
        None => {
            // No output specified: use export filename in current directory
            std::path::PathBuf::from(&export_filename)
        }
    };

    // Validate the final resolved path -- the remote filename is untrusted
    // (from Drive API) and could contain path traversal segments.
    crate::validate::validate_safe_file_path(&dest.to_string_lossy(), "output path")?;

    // Step 2: Export the document
    let export_url = format!(
        "https://www.googleapis.com/drive/v3/files/{}/export",
        encoded_id,
    );
    let export_resp = crate::client::send_with_retry(|| {
        client
            .get(&export_url)
            .query(&[("mimeType", mime_type)])
            .bearer_auth(&token)
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Export request failed: {e}")))?;

    if !export_resp.status().is_success() {
        let status = export_resp.status();
        let body = export_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "export_failed".to_string(),
            enable_url: None,
        });
    }

    let total_bytes = stream_to_file(export_resp, &dest).await?;

    let result = json!({
        "status": "success",
        "file": dest.display().to_string(),
        "mimeType": mime_type,
        "format": format,
        "bytes": total_bytes,
        "sourceFileId": file_id,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );

    Ok(())
}

/// Map a user-friendly format name to the corresponding MIME type for Drive export.
fn format_to_mime(format: &str) -> Result<&'static str, GwsError> {
    match format.to_lowercase().as_str() {
        // Documents
        "pdf" => Ok("application/pdf"),
        "docx" => Ok("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "odt" => Ok("application/vnd.oasis.opendocument.text"),
        "rtf" => Ok("application/rtf"),
        "txt" | "text" => Ok("text/plain"),
        "html" => Ok("text/html"),
        "epub" => Ok("application/epub+zip"),
        "md" | "markdown" => Ok("text/markdown"),
        "zip" => Ok("application/zip"),
        // Spreadsheets
        "xlsx" => Ok("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ods" => Ok("application/vnd.oasis.opendocument.spreadsheet"),
        "csv" => Ok("text/csv"),
        "tsv" => Ok("text/tab-separated-values"),
        // Presentations
        "pptx" => Ok("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "odp" => Ok("application/vnd.oasis.opendocument.presentation"),
        // Images (for Drawings)
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "svg" => Ok("image/svg+xml"),
        _ => Err(GwsError::Validation(format!(
            "Unsupported export format: '{}'. Supported: pdf, docx, odt, rtf, txt, html, epub, md, zip, xlsx, ods, csv, tsv, pptx, odp, png, jpg, svg",
            crate::output::sanitize_for_terminal(format)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_to_mime_documents() {
        assert_eq!(format_to_mime("pdf").unwrap(), "application/pdf");
        assert_eq!(
            format_to_mime("docx").unwrap(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(
            format_to_mime("odt").unwrap(),
            "application/vnd.oasis.opendocument.text"
        );
        assert_eq!(format_to_mime("rtf").unwrap(), "application/rtf");
        assert_eq!(format_to_mime("txt").unwrap(), "text/plain");
        assert_eq!(format_to_mime("text").unwrap(), "text/plain");
        assert_eq!(format_to_mime("html").unwrap(), "text/html");
        assert_eq!(format_to_mime("epub").unwrap(), "application/epub+zip");
        assert_eq!(format_to_mime("md").unwrap(), "text/markdown");
        assert_eq!(format_to_mime("markdown").unwrap(), "text/markdown");
        assert_eq!(format_to_mime("zip").unwrap(), "application/zip");
    }

    #[test]
    fn test_format_to_mime_spreadsheets() {
        assert_eq!(
            format_to_mime("xlsx").unwrap(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(
            format_to_mime("ods").unwrap(),
            "application/vnd.oasis.opendocument.spreadsheet"
        );
        assert_eq!(format_to_mime("csv").unwrap(), "text/csv");
        assert_eq!(
            format_to_mime("tsv").unwrap(),
            "text/tab-separated-values"
        );
    }

    #[test]
    fn test_format_to_mime_presentations() {
        assert_eq!(
            format_to_mime("pptx").unwrap(),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        );
        assert_eq!(
            format_to_mime("odp").unwrap(),
            "application/vnd.oasis.opendocument.presentation"
        );
    }

    #[test]
    fn test_format_to_mime_images() {
        assert_eq!(format_to_mime("png").unwrap(), "image/png");
        assert_eq!(format_to_mime("jpg").unwrap(), "image/jpeg");
        assert_eq!(format_to_mime("jpeg").unwrap(), "image/jpeg");
        assert_eq!(format_to_mime("svg").unwrap(), "image/svg+xml");
    }

    #[test]
    fn test_format_to_mime_case_insensitive() {
        assert_eq!(format_to_mime("PDF").unwrap(), "application/pdf");
        assert_eq!(format_to_mime("Docx").unwrap(), format_to_mime("docx").unwrap());
    }

    #[test]
    fn test_format_to_mime_unsupported() {
        assert!(format_to_mime("mp4").is_err());
        assert!(format_to_mime("").is_err());
        assert!(format_to_mime("unknown").is_err());
    }

    #[test]
    fn test_export_command_requires_format() {
        use clap::{Arg, Command};
        let cmd = Command::new("export")
            .arg(Arg::new("file-id").required(true).index(1))
            .arg(Arg::new("format").long("format").short('f').required(true));
        assert!(cmd.try_get_matches_from(["export", "abc123"]).is_err());
    }

    #[test]
    fn test_export_command_parses_args() {
        use clap::{Arg, Command};
        let cmd = Command::new("export")
            .arg(Arg::new("file-id").required(true).index(1))
            .arg(Arg::new("format").long("format").short('f').required(true))
            .arg(Arg::new("output").long("output").short('o'));
        let m = cmd
            .try_get_matches_from(["export", "abc123", "--format", "pdf", "-o", "out.pdf"])
            .unwrap();
        assert_eq!(m.get_one::<String>("file-id").unwrap(), "abc123");
        assert_eq!(m.get_one::<String>("format").unwrap(), "pdf");
        assert_eq!(m.get_one::<String>("output").unwrap(), "out.pdf");
    }
}

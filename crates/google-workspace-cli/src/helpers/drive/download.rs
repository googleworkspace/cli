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

/// Handle the `+download` subcommand.
pub(super) async fn handle_download(matches: &ArgMatches) -> Result<(), GwsError> {
    let file_id = matches.get_one::<String>("file-id").unwrap();
    let output_path = matches.get_one::<String>("output");

    // Validate output path before any network I/O
    if let Some(p) = output_path {
        crate::validate::validate_safe_file_path(p, "--output")?;
    }

    let dry_run = matches.get_flag("dry-run");

    if dry_run {
        let info = json!({
            "dry_run": true,
            "action": "download",
            "file_id": file_id,
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

    // Step 1: Fetch metadata for the real filename and MIME type
    let metadata = fetch_file_metadata(&client, &token, &encoded_id, "name,mimeType,size").await?;

    let remote_name = metadata
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("download");

    // Guard: Google Workspace files cannot be downloaded directly
    if let Some(mime) = metadata.get("mimeType").and_then(|v| v.as_str()) {
        if mime.starts_with(GOOGLE_APPS_MIME_PREFIX) {
            return Err(GwsError::Validation(format!(
                "File '{}' is a Google Workspace document ({}). \
                 Use `gws drive +export` to export it to a local format (e.g., --format pdf).",
                remote_name, mime
            )));
        }
    }

    // Determine output file path
    let dest = resolve_output_path(output_path.map(|s| s.as_str()), remote_name)?;

    // Step 2: Download binary content
    let download_url = format!(
        "https://www.googleapis.com/drive/v3/files/{}",
        encoded_id,
    );
    let download_resp = crate::client::send_with_retry(|| {
        client
            .get(&download_url)
            .query(&[("alt", "media")])
            .bearer_auth(&token)
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Download request failed: {e}")))?;

    if !download_resp.status().is_success() {
        let status = download_resp.status();
        let body = download_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "download_failed".to_string(),
            enable_url: None,
        });
    }

    let total_bytes = stream_to_file(download_resp, &dest).await?;

    let result = json!({
        "status": "success",
        "file": dest.display().to_string(),
        "bytes": total_bytes,
        "sourceFileId": file_id,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::{Arg, Command};

    fn download_cmd() -> Command {
        Command::new("download")
            .arg(Arg::new("file-id").required(true).index(1))
            .arg(Arg::new("output").long("output").short('o').value_name("PATH"))
    }

    #[test]
    fn test_download_command_requires_file_id() {
        assert!(download_cmd().try_get_matches_from(["download"]).is_err());
    }

    #[test]
    fn test_download_command_parses_args() {
        let m = download_cmd()
            .try_get_matches_from(["download", "abc123", "--output", "out.pdf"])
            .unwrap();
        assert_eq!(m.get_one::<String>("file-id").unwrap(), "abc123");
        assert_eq!(m.get_one::<String>("output").unwrap(), "out.pdf");
    }
}

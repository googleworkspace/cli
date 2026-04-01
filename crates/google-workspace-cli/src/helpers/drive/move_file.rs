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

/// Handle the `+move` subcommand.
pub(super) async fn handle_move(matches: &ArgMatches) -> Result<(), GwsError> {
    let file_id = matches.get_one::<String>("file-id").unwrap();
    let dest_folder = matches.get_one::<String>("to").unwrap();

    let dry_run = matches.get_flag("dry-run");

    if dry_run {
        let info = json!({
            "dry_run": true,
            "action": "move",
            "file_id": file_id,
            "destination": dest_folder,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&info).unwrap_or_default()
        );
        return Ok(());
    }

    let token = auth::get_token(&[DRIVE_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Drive auth failed: {e}")))?;

    let client = crate::client::build_client()?;
    let encoded_id = encode_path_segment(file_id);

    // Step 1: Get current parents
    let metadata = fetch_file_metadata(&client, &token, &encoded_id, "id,name,parents").await?;

    let current_parents: Vec<&str> = metadata
        .get("parents")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let remove_parents = current_parents.join(",");

    // Step 2: Move by updating parents
    let update_url = format!(
        "https://www.googleapis.com/drive/v3/files/{}",
        encoded_id,
    );

    let update_resp = crate::client::send_with_retry(|| {
        client
            .patch(&update_url)
            .query(&[
                ("addParents", dest_folder.as_str()),
                ("removeParents", remove_parents.as_str()),
                ("fields", "id,name,parents"),
            ])
            .bearer_auth(&token)
            .header("Content-Type", "application/json")
            .body("{}")
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Move request failed: {e}")))?;

    if !update_resp.status().is_success() {
        let status = update_resp.status();
        let body = update_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "move_failed".to_string(),
            enable_url: None,
        });
    }

    let result: Value = update_resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse move response: {e}")))?;

    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::{Arg, Command};

    fn move_cmd() -> Command {
        Command::new("move")
            .arg(Arg::new("file-id").required(true).index(1))
            .arg(Arg::new("to").long("to").required(true))
    }

    #[test]
    fn test_move_command_requires_to() {
        assert!(move_cmd().try_get_matches_from(["move", "abc123"]).is_err());
    }

    #[test]
    fn test_move_command_parses_args() {
        let m = move_cmd()
            .try_get_matches_from(["move", "abc123", "--to", "folder456"])
            .unwrap();
        assert_eq!(m.get_one::<String>("file-id").unwrap(), "abc123");
        assert_eq!(m.get_one::<String>("to").unwrap(), "folder456");
    }
}

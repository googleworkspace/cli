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

//! Gmail `+triage` helper — lists unread messages with sender, subject, date.
//!
//! Read-only: fetches unread message metadata (From, Subject, Date) and
//! optionally includes label IDs. Supports custom Gmail search queries
//! via `--query` and configurable result limits via `--max`.

use super::*;

/// Handle the `+triage` subcommand.
pub async fn handle_triage(matches: &ArgMatches) -> Result<(), GwsError> {
    let max: u32 = matches
        .get_one::<String>("max")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let query = matches
        .get_one::<String>("query")
        .map(|s| s.as_str())
        .unwrap_or("is:unread");
    let show_labels = matches.get_flag("labels");
    let output_format = matches
        .get_one::<String>("format")
        .map(|s| crate::formatter::OutputFormat::from_str(s))
        .unwrap_or(crate::formatter::OutputFormat::Table);

    // Authenticate — use gmail.readonly instead of gmail.modify because triage
    // is read-only and the `q` query parameter is not supported under the
    // gmail.metadata scope.  When a token carries both metadata and modify
    // scopes the API may resolve to the metadata path and reject `q` with 403.
    // gmail.readonly always supports `q`.
    let token = auth::get_token(&[GMAIL_READONLY_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // 1. List message IDs
    let list_url = "https://gmail.googleapis.com/gmail/v1/users/me/messages";

    let list_resp = client
        .get(list_url)
        .query(&[("q", query), ("maxResults", &max.to_string())])
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to list messages: {e}")))?;

    if !list_resp.status().is_success() {
        let err = list_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: 0,
            message: err,
            reason: "list_failed".to_string(),
            enable_url: None,
        });
    }

    let list_json: Value = list_resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse list response: {e}")))?;

    let messages = match list_json.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => {
            eprintln!("{}", no_messages_msg(query));
            return Ok(());
        }
    };

    if messages.is_empty() {
        eprintln!("{}", no_messages_msg(query));
        return Ok(());
    }

    // 2. Fetch metadata for each message concurrently
    use futures_util::stream::{self, StreamExt};

    // Collect message IDs upfront (owned) to avoid lifetime issues in async closures
    let msg_ids: Vec<String> = messages
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    let results: Vec<Value> = stream::iter(msg_ids)
        .map(|msg_id| {
            let client = &client;
            let token = &token;
            async move {
                let get_url = format!(
                    "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
                    msg_id
                );

                let get_resp = crate::client::send_with_retry(|| {
                    client.get(&get_url).bearer_auth(token)
                })
                .await
                .ok()?;

                if !get_resp.status().is_success() {
                    return None;
                }

                let msg_json: Value = get_resp.json().await.ok()?;

                let headers = msg_json
                    .get("payload")
                    .and_then(|p| p.get("headers"))
                    .and_then(|h| h.as_array());

                let mut from = String::new();
                let mut subject = String::new();
                let mut date = String::new();

                if let Some(headers) = headers {
                    for h in headers {
                        let name = h.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
                        match name {
                            "From" => from = value.to_string(),
                            "Subject" => subject = value.to_string(),
                            "Date" => date = value.to_string(),
                            _ => {}
                        }
                    }
                }

                let mut entry = json!({
                    "id": msg_id,
                    "from": from,
                    "subject": subject,
                    "date": date,
                });

                if show_labels {
                    let labels = msg_json
                        .get("labelIds")
                        .cloned()
                        .unwrap_or(Value::Array(vec![]));
                    entry["labels"] = labels;
                }

                Some(entry)
            }
        })
        .buffer_unordered(10)
        .filter_map(|r| async { r })
        .collect()
        .await;

    // 3. Output
    let result_count = results.len();
    let output = json!({
        "messages": results,
        "resultSizeEstimate": list_json.get("resultSizeEstimate").cloned().unwrap_or(json!(result_count)),
        "query": query,
    });

    println!(
        "{}",
        crate::formatter::format_value(&output, &output_format)
    );

    Ok(())
}

/// Returns the human-readable "no messages" diagnostic string.
/// Extracted so the test can reference the exact same message without duplication.
fn no_messages_msg(query: &str) -> String {
    format!("No messages found matching query: {query}")
}

#[cfg(test)]
mod tests {
    use super::no_messages_msg;
    use clap::{Arg, ArgAction, Command};

    /// Build a clap command matching the +triage definition so we can
    /// unit-test argument parsing without needing a live GmailHelper.
    fn triage_cmd() -> Command {
        Command::new("triage")
            .arg(
                Arg::new("max")
                    .long("max")
                    .default_value("20")
                    .value_name("N"),
            )
            .arg(Arg::new("query").long("query").value_name("QUERY"))
            .arg(Arg::new("labels").long("labels").action(ArgAction::SetTrue))
            .arg(Arg::new("format").long("format").value_name("FMT"))
    }

    #[test]
    fn defaults_max_to_20_and_query_to_unread() {
        let m = triage_cmd().try_get_matches_from(["triage"]).unwrap();
        let max: u32 = m
            .get_one::<String>("max")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let query = m
            .get_one::<String>("query")
            .map(|s| s.as_str())
            .unwrap_or("is:unread");
        assert_eq!(max, 20);
        assert_eq!(query, "is:unread");
    }

    #[test]
    fn explicit_max_overrides_default() {
        let m = triage_cmd()
            .try_get_matches_from(["triage", "--max", "5"])
            .unwrap();
        let max: u32 = m
            .get_one::<String>("max")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        assert_eq!(max, 5);
    }

    #[test]
    fn non_numeric_max_falls_back_to_20() {
        let m = triage_cmd()
            .try_get_matches_from(["triage", "--max", "abc"])
            .unwrap();
        let max: u32 = m
            .get_one::<String>("max")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        assert_eq!(max, 20);
    }

    #[test]
    fn custom_query_overrides_default() {
        let m = triage_cmd()
            .try_get_matches_from(["triage", "--query", "from:boss"])
            .unwrap();
        let query = m
            .get_one::<String>("query")
            .map(|s| s.as_str())
            .unwrap_or("is:unread");
        assert_eq!(query, "from:boss");
    }

    #[test]
    fn labels_flag_defaults_to_false() {
        let m = triage_cmd().try_get_matches_from(["triage"]).unwrap();
        assert!(!m.get_flag("labels"));
    }

    #[test]
    fn labels_flag_set_when_passed() {
        let m = triage_cmd()
            .try_get_matches_from(["triage", "--labels"])
            .unwrap();
        assert!(m.get_flag("labels"));
    }

    #[test]
    fn format_defaults_to_table_when_absent() {
        let m = triage_cmd().try_get_matches_from(["triage"]).unwrap();
        let fmt = m
            .get_one::<String>("format")
            .map(|s| crate::formatter::OutputFormat::from_str(s))
            .unwrap_or(crate::formatter::OutputFormat::Table);
        assert!(matches!(fmt, crate::formatter::OutputFormat::Table));
    }

    #[test]
    fn format_json_when_specified() {
        let m = triage_cmd()
            .try_get_matches_from(["triage", "--format", "json"])
            .unwrap();
        let fmt = m
            .get_one::<String>("format")
            .map(|s| crate::formatter::OutputFormat::from_str(s))
            .unwrap_or(crate::formatter::OutputFormat::Table);
        assert!(matches!(fmt, crate::formatter::OutputFormat::Json));
    }

    #[test]
    fn empty_result_message_is_not_json() {
        // Verify that no_messages_msg() produces a human-readable string that
        // belongs on stderr, not stdout. If it were valid JSON it could corrupt
        // pipe workflows like `gws gmail +triage | jq`.
        let msg = no_messages_msg("label:inbox");
        assert!(serde_json::from_str::<serde_json::Value>(&msg).is_err());
    }
}

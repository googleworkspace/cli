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

    // Authenticate
    let token = auth::get_token(&[GMAIL_SCOPE], None)
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
            println!("No messages found matching query: {query}");
            return Ok(());
        }
    };

    if messages.is_empty() {
        println!("No messages found matching query: {query}");
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

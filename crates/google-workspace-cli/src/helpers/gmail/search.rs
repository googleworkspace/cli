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

//! Gmail `+search` helper — searches messages with full metadata output.
//!
//! Multi-step orchestration: `messages.list` → `labels.list` (account-wide
//! ID→name map) → concurrent `messages.get` (`format=metadata`).

use std::collections::HashMap;

use futures_util::stream::{self, StreamExt};

use super::*;

/// A single search result with resolved labels and parsed address headers.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResult {
    id: String,
    thread_id: String,
    from: Mailbox,
    to: Vec<Mailbox>,
    cc: Vec<Mailbox>,
    subject: String,
    date: String,
    snippet: String,
    labels: Vec<Label>,
}

/// Top-level search response envelope.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    messages: Vec<SearchResult>,
    /// Rough estimate of total matching messages. Gmail API caveat: can be
    /// significantly inaccurate for broad queries. Not suitable for precise
    /// pagination math.
    result_size_estimate: u64,
    query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page_token: Option<String>,
}

/// Handle the `+search` subcommand: query messages, resolve labels, and print
/// a JSON envelope to stdout.
pub(super) async fn handle_search(matches: &ArgMatches) -> Result<(), GwsError> {
    let max = matches.get_one::<u32>("max").copied().unwrap_or(20);
    let query = matches
        .get_one::<String>("query")
        .map(String::as_str)
        .expect("--query is required");
    let page_token = matches.get_one::<String>("page-token");
    let output_format = matches
        .get_one::<String>("format")
        .map(|s| crate::formatter::OutputFormat::from_str(s))
        .unwrap_or(crate::formatter::OutputFormat::Json);

    let token = auth::get_token(&[GMAIL_READONLY_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // 1. List message IDs matching the query
    let list_url = "https://gmail.googleapis.com/gmail/v1/users/me/messages";
    let max_str = max.to_string();
    let mut query_params: Vec<(&str, &str)> = vec![("q", query), ("maxResults", &max_str)];
    if let Some(pt) = page_token {
        query_params.push(("pageToken", pt));
    }

    let list_resp = crate::client::send_with_retry(|| {
        client
            .get(list_url)
            .query(&query_params)
            .bearer_auth(&token)
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to list messages: {e}")))?;

    if !list_resp.status().is_success() {
        let status = list_resp.status().as_u16();
        let body = list_resp
            .text()
            .await
            .unwrap_or_else(|_| "(error body unreadable)".to_string());
        return Err(build_api_error(status, &body, "Failed to list messages"));
    }

    let list_json: Value = list_resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse list response: {e}")))?;

    let query_owned = query.to_string();

    let messages = match list_json.get("messages").and_then(|m| m.as_array()) {
        Some(m) if !m.is_empty() => m,
        _ => {
            // Output a valid empty response so machine consumers always get parseable JSON.
            let response = SearchResponse {
                messages: vec![],
                result_size_estimate: 0,
                query: query_owned,
                next_page_token: None,
            };
            let output = serde_json::to_value(&response)
                .map_err(|e| GwsError::Other(anyhow::anyhow!("{e}")))?;
            println!(
                "{}",
                crate::formatter::format_value(&output, &output_format)
            );
            return Ok(());
        }
    };

    // 2. Fetch label ID→name map
    let label_map = fetch_label_map(&client, &token).await?;

    // 3. Fetch metadata for each message concurrently.
    //    buffered() (not buffer_unordered) preserves the API's relevance-ranked order.
    let msg_ids: Vec<String> = messages
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    let fetch_results: Vec<Result<SearchResult, GwsError>> = stream::iter(msg_ids)
        .map(|msg_id| {
            let client = &client;
            let token = &token;
            let label_map = &label_map;
            async move { fetch_search_result(client, token, &msg_id, label_map).await }
        })
        .buffered(10)
        .collect()
        .await;

    // Partition into successes and failures. Infrastructure errors (auth, network)
    // abort the search; per-message parse failures are skipped with a warning.
    let mut results = Vec::with_capacity(fetch_results.len());
    let mut skipped = 0u32;
    for item in fetch_results {
        match item {
            Ok(result) => results.push(result),
            Err(e) if is_per_message_error(&e) => {
                skipped += 1;
                crate::output::warn(&format!("skipping message: {}", crate::output::sanitize_for_terminal(&e.to_string())));
            }
            Err(e) => return Err(e),
        }
    }
    if skipped > 0 {
        crate::output::warn(&format!(
            "{skipped} message(s) could not be fetched and were skipped"
        ));
    }

    // 4. Build response envelope and output
    let response = SearchResponse {
        result_size_estimate: list_json
            .get("resultSizeEstimate")
            .and_then(|v| v.as_u64())
            .unwrap_or(results.len() as u64),
        next_page_token: list_json
            .get("nextPageToken")
            .and_then(|v| v.as_str())
            .map(String::from),
        messages: results,
        query: query_owned,
    };

    let output = serde_json::to_value(&response)
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to serialize response: {e}")))?;

    println!(
        "{}",
        crate::formatter::format_value(&output, &output_format)
    );

    // For non-JSON formats, the formatter may strip envelope fields (nextPageToken,
    // resultSizeEstimate) when it finds the messages array. Print a stderr hint so
    // users can still paginate.
    if let Some(token) = &response.next_page_token {
        if !matches!(output_format, crate::formatter::OutputFormat::Json) {
            crate::output::info(&format!(
                "More results available. Continue with: --page-token {}",
                crate::output::sanitize_for_terminal(token)
            ));
        }
    }

    Ok(())
}

/// Classify whether an error is a per-message issue (skip) or an infrastructure
/// issue (abort).
///
/// Per-message (skip): HTTP 404/410 (message deleted), `Validation` (malformed
/// message metadata from `parse_search_result`).
///
/// Infrastructure (abort): auth failures (401/403), rate limits (429), server
/// errors (5xx), and all other variants (`Other`, `Auth`, `Discovery`).
fn is_per_message_error(e: &GwsError) -> bool {
    match e {
        GwsError::Api { code, .. } => matches!(*code, 404 | 410),
        GwsError::Validation(_) => true,
        _ => false,
    }
}

/// Fetch all Gmail labels and return a map of label ID → label name.
async fn fetch_label_map(
    client: &reqwest::Client,
    token: &str,
) -> Result<HashMap<String, String>, GwsError> {
    let url = "https://gmail.googleapis.com/gmail/v1/users/me/labels";

    let resp = crate::client::send_with_retry(|| client.get(url).bearer_auth(token))
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch labels: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|_| "(error body unreadable)".to_string());
        return Err(build_api_error(status, &body, "Failed to fetch labels"));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse labels response: {e}")))?;

    Ok(json
        .get("labels")
        .and_then(|l| l.as_array())
        .map(|labels| {
            labels
                .iter()
                .filter_map(|label| {
                    let id = label.get("id").and_then(|v| v.as_str())?;
                    let name = label.get("name").and_then(|v| v.as_str())?;
                    Some((id.to_string(), name.to_string()))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Fetch a single message's metadata and parse it into a `SearchResult`.
async fn fetch_search_result(
    client: &reqwest::Client,
    token: &str,
    msg_id: &str,
    label_map: &HashMap<String, String>,
) -> Result<SearchResult, GwsError> {
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}",
        crate::validate::encode_path_segment(msg_id)
    );

    let resp = crate::client::send_with_retry(|| {
        client.get(&url).bearer_auth(token).query(&[
            ("format", "metadata"),
            ("metadataHeaders", "From"),
            ("metadataHeaders", "To"),
            ("metadataHeaders", "Cc"),
            ("metadataHeaders", "Subject"),
            ("metadataHeaders", "Date"),
        ])
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch message {msg_id}: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|_| "(error body unreadable)".to_string());
        return Err(build_api_error(
            status,
            &body,
            &format!("Failed to fetch message {msg_id}"),
        ));
    }

    let msg_json: Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse message {msg_id}: {e}")))?;

    parse_search_result(&msg_json, label_map)
}

/// Parse a Gmail API message (format=metadata) JSON into a `SearchResult`.
fn parse_search_result(
    msg: &Value,
    label_map: &HashMap<String, String>,
) -> Result<SearchResult, GwsError> {
    let id = msg
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GwsError::Validation("Message missing 'id' field".to_string()))?
        .to_string();

    let thread_id = msg
        .get("threadId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GwsError::Validation(format!("Message {id} missing 'threadId' field")))?
        .to_string();

    let snippet = msg
        .get("snippet")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let headers = msg
        .get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array())
        .ok_or_else(|| GwsError::Validation(format!("Message {id} missing payload headers")))?;

    let parsed = parse_message_headers(headers);

    if parsed.from.is_empty() {
        return Err(GwsError::Validation(format!(
            "Message {id} missing From header"
        )));
    }

    let labels = msg
        .get("labelIds")
        .and_then(|v| v.as_array())
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str())
                .map(|id_str| Label::resolve(id_str, label_map))
                .collect()
        })
        .unwrap_or_default();

    Ok(SearchResult {
        id,
        thread_id,
        from: Mailbox::parse(&parsed.from),
        to: Mailbox::parse_list(&parsed.to),
        cc: Mailbox::parse_list(&parsed.cc),
        subject: parsed.subject,
        date: parsed.date,
        snippet,
        labels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Arg, Command};
    use serde_json::json;
    use std::collections::HashMap;

    fn search_cmd() -> Command {
        Command::new("search")
            .arg(
                Arg::new("query")
                    .long("query")
                    .required(true)
                    .value_name("QUERY"),
            )
            .arg(
                Arg::new("max")
                    .long("max")
                    .default_value("20")
                    .value_parser(clap::value_parser!(u32).range(1..=500))
                    .value_name("N"),
            )
            .arg(
                Arg::new("page-token")
                    .long("page-token")
                    .value_name("TOKEN"),
            )
            .arg(Arg::new("format").long("format").value_name("FMT"))
    }

    #[test]
    fn search_requires_query() {
        assert!(search_cmd().try_get_matches_from(["search"]).is_err());
    }

    #[test]
    fn explicit_query_is_accepted() {
        let m = search_cmd()
            .try_get_matches_from(["search", "--query", "from:boss"])
            .unwrap();
        let query = m.get_one::<String>("query").unwrap().as_str();
        assert_eq!(query, "from:boss");
    }

    #[test]
    fn defaults_max_to_20() {
        let m = search_cmd()
            .try_get_matches_from(["search", "--query", "test"])
            .unwrap();
        let max = m.get_one::<u32>("max").copied().unwrap_or(20);
        assert_eq!(max, 20);
    }

    #[test]
    fn explicit_max_overrides_default() {
        let m = search_cmd()
            .try_get_matches_from(["search", "--query", "test", "--max", "5"])
            .unwrap();
        let max = m.get_one::<u32>("max").copied().unwrap_or(20);
        assert_eq!(max, 5);
    }

    #[test]
    fn non_numeric_max_is_rejected_by_clap() {
        assert!(search_cmd()
            .try_get_matches_from(["search", "--query", "test", "--max", "abc"])
            .is_err());
    }

    #[test]
    fn max_over_500_is_rejected() {
        assert!(search_cmd()
            .try_get_matches_from(["search", "--query", "test", "--max", "501"])
            .is_err());
    }

    #[test]
    fn max_zero_is_rejected() {
        assert!(search_cmd()
            .try_get_matches_from(["search", "--query", "test", "--max", "0"])
            .is_err());
    }

    #[test]
    fn page_token_is_optional() {
        let m = search_cmd()
            .try_get_matches_from(["search", "--query", "test"])
            .unwrap();
        assert!(m.get_one::<String>("page-token").is_none());
    }

    #[test]
    fn page_token_accepted_when_provided() {
        let m = search_cmd()
            .try_get_matches_from(["search", "--query", "test", "--page-token", "abc123"])
            .unwrap();
        assert_eq!(
            m.get_one::<String>("page-token").unwrap().as_str(),
            "abc123"
        );
    }

    #[test]
    fn format_defaults_to_json() {
        let m = search_cmd()
            .try_get_matches_from(["search", "--query", "test"])
            .unwrap();
        let fmt = m
            .get_one::<String>("format")
            .map(|s| crate::formatter::OutputFormat::from_str(s))
            .unwrap_or(crate::formatter::OutputFormat::Json);
        assert!(matches!(fmt, crate::formatter::OutputFormat::Json));
    }

    #[test]
    fn format_table_when_specified() {
        let m = search_cmd()
            .try_get_matches_from(["search", "--query", "test", "--format", "table"])
            .unwrap();
        let fmt = m
            .get_one::<String>("format")
            .map(|s| crate::formatter::OutputFormat::from_str(s))
            .unwrap_or(crate::formatter::OutputFormat::Json);
        assert!(matches!(fmt, crate::formatter::OutputFormat::Table));
    }

    // --- parse_search_result tests ---

    fn make_msg(overrides: Value) -> Value {
        let mut base = json!({
            "id": "abc123",
            "threadId": "thread456",
            "snippet": "Hello world",
            "labelIds": [],
            "payload": {
                "headers": [
                    {"name": "From", "value": "Alice <alice@example.com>"},
                    {"name": "To", "value": "bob@example.com"},
                    {"name": "Subject", "value": "Test"},
                    {"name": "Date", "value": "Thu, 26 Mar 2026 10:00:00 -0400"},
                ]
            }
        });
        if let (Value::Object(base_map), Value::Object(overrides_map)) = (&mut base, overrides) {
            for (key, value) in overrides_map {
                base_map.insert(key, value);
            }
        }
        base
    }

    #[test]
    fn parse_search_result_happy_path() {
        let msg = json!({
            "id": "abc123",
            "threadId": "thread456",
            "snippet": "Hello world",
            "labelIds": ["INBOX", "Label_42"],
            "payload": {
                "headers": [
                    {"name": "From", "value": "Alice <alice@example.com>"},
                    {"name": "To", "value": "bob@example.com, dave@example.com"},
                    {"name": "Cc", "value": "carol@example.com"},
                    {"name": "Subject", "value": "Test subject"},
                    {"name": "Date", "value": "Thu, 26 Mar 2026 10:00:00 -0400"},
                ]
            }
        });

        let mut label_map = HashMap::new();
        label_map.insert("INBOX".to_string(), "INBOX".to_string());
        label_map.insert("Label_42".to_string(), "Projects/Alpha".to_string());

        let result = parse_search_result(&msg, &label_map).unwrap();
        assert_eq!(result.id, "abc123");
        assert_eq!(result.thread_id, "thread456");
        assert_eq!(result.from.email, "alice@example.com");
        assert_eq!(result.from.name, Some("Alice".to_string()));
        assert_eq!(result.to.len(), 2);
        assert_eq!(result.to[0].email, "bob@example.com");
        assert_eq!(result.to[1].email, "dave@example.com");
        assert_eq!(result.cc.len(), 1);
        assert_eq!(result.cc[0].email, "carol@example.com");
        assert_eq!(result.subject, "Test subject");
        assert_eq!(result.snippet, "Hello world");
        assert_eq!(result.labels.len(), 2);
        assert_eq!(result.labels[0].name, "INBOX");
        assert_eq!(result.labels[1].id, "Label_42");
        assert_eq!(result.labels[1].name, "Projects/Alpha");
    }

    #[test]
    fn parse_search_result_missing_id_returns_err() {
        let mut msg = make_msg(json!({}));
        msg.as_object_mut().unwrap().remove("id");
        assert!(parse_search_result(&msg, &HashMap::new()).is_err());
    }

    #[test]
    fn parse_search_result_missing_thread_id_returns_err() {
        let mut msg = make_msg(json!({}));
        msg.as_object_mut().unwrap().remove("threadId");
        assert!(parse_search_result(&msg, &HashMap::new()).is_err());
    }

    #[test]
    fn parse_search_result_missing_headers_returns_err() {
        let msg = json!({
            "id": "abc",
            "threadId": "t1",
            "snippet": "",
        });
        assert!(parse_search_result(&msg, &HashMap::new()).is_err());
    }

    #[test]
    fn parse_search_result_empty_from_returns_err() {
        let msg = json!({
            "id": "abc",
            "threadId": "t1",
            "snippet": "",
            "payload": {
                "headers": [
                    {"name": "Subject", "value": "No From header"},
                ]
            }
        });
        assert!(parse_search_result(&msg, &HashMap::new()).is_err());
    }

    #[test]
    fn parse_search_result_empty_headers_returns_err() {
        let msg = json!({
            "id": "abc",
            "threadId": "t1",
            "snippet": "",
            "payload": {
                "headers": []
            }
        });
        assert!(parse_search_result(&msg, &HashMap::new()).is_err());
    }

    #[test]
    fn parse_search_result_unknown_label_uses_id_as_name() {
        let msg = make_msg(json!({"labelIds": ["UNKNOWN_LABEL"]}));
        let result = parse_search_result(&msg, &HashMap::new()).unwrap();
        assert_eq!(result.labels[0].id, "UNKNOWN_LABEL");
        assert_eq!(result.labels[0].name, "UNKNOWN_LABEL");
    }

    #[test]
    fn parse_search_result_empty_labels() {
        let msg = make_msg(json!({"labelIds": []}));
        let result = parse_search_result(&msg, &HashMap::new()).unwrap();
        assert!(result.labels.is_empty());
    }

    #[test]
    fn parse_search_result_no_label_ids_field() {
        let mut msg = make_msg(json!({}));
        msg.as_object_mut().unwrap().remove("labelIds");
        let result = parse_search_result(&msg, &HashMap::new()).unwrap();
        assert!(result.labels.is_empty());
    }

    // --- serialization tests ---

    #[test]
    fn search_result_camel_case_serialization() {
        let result = parse_search_result(&make_msg(json!({})), &HashMap::new()).unwrap();
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("threadId").is_some());
        assert!(json.get("thread_id").is_none());
    }

    #[test]
    fn search_response_camel_case_serialization() {
        let response = SearchResponse {
            messages: vec![],
            result_size_estimate: 0,
            query: "test".to_string(),
            next_page_token: Some("token123".to_string()),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert!(json.get("resultSizeEstimate").is_some());
        assert!(json.get("nextPageToken").is_some());
        // snake_case variants must not be present
        assert!(json.get("result_size_estimate").is_none());
        assert!(json.get("next_page_token").is_none());
    }

    #[test]
    fn search_response_omits_null_page_token() {
        let response = SearchResponse {
            messages: vec![],
            result_size_estimate: 0,
            query: "test".to_string(),
            next_page_token: None,
        };
        let json = serde_json::to_value(&response).unwrap();
        assert!(json.get("nextPageToken").is_none());
    }

    // --- error classification tests ---

    #[test]
    fn auth_errors_are_not_per_message() {
        let e = GwsError::Api {
            code: 401,
            message: "unauthorized".to_string(),
            reason: "authError".to_string(),
            enable_url: None,
        };
        assert!(!is_per_message_error(&e));
    }

    #[test]
    fn forbidden_errors_are_not_per_message() {
        let e = GwsError::Api {
            code: 403,
            message: "forbidden".to_string(),
            reason: "forbidden".to_string(),
            enable_url: None,
        };
        assert!(!is_per_message_error(&e));
    }

    #[test]
    fn not_found_errors_are_per_message() {
        let e = GwsError::Api {
            code: 404,
            message: "not found".to_string(),
            reason: "notFound".to_string(),
            enable_url: None,
        };
        assert!(is_per_message_error(&e));
    }

    #[test]
    fn gone_errors_are_per_message() {
        let e = GwsError::Api {
            code: 410,
            message: "gone".to_string(),
            reason: "gone".to_string(),
            enable_url: None,
        };
        assert!(is_per_message_error(&e));
    }

    #[test]
    fn validation_errors_are_per_message() {
        let e = GwsError::Validation("Missing From header".to_string());
        assert!(is_per_message_error(&e));
    }

    #[test]
    fn rate_limit_errors_are_not_per_message() {
        let e = GwsError::Api {
            code: 429,
            message: "rate limited".to_string(),
            reason: "rateLimitExceeded".to_string(),
            enable_url: None,
        };
        assert!(!is_per_message_error(&e));
    }

    #[test]
    fn server_errors_are_not_per_message() {
        let e = GwsError::Api {
            code: 500,
            message: "internal".to_string(),
            reason: "backendError".to_string(),
            enable_url: None,
        };
        assert!(!is_per_message_error(&e));
    }

    #[test]
    fn transport_errors_are_not_per_message() {
        let e = GwsError::Other(anyhow::anyhow!("network timeout"));
        assert!(!is_per_message_error(&e));
    }
}

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

//! Gmail `+reply` helper — reply to an existing message with proper threading.

use super::*;

/// Handle the `+reply` subcommand.
pub async fn handle_reply(matches: &ArgMatches) -> Result<(), GwsError> {
    let message_id = matches.get_one::<String>("to").unwrap();
    let body_text = matches.get_one::<String>("body").unwrap();
    let reply_all = matches.get_flag("all");

    // Authenticate
    let token = auth::get_token(&[GMAIL_SCOPE], None)
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // 1. Fetch original message metadata
    let encoded_id = crate::validate::encode_path_segment(message_id);
    let get_url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{encoded_id}?format=full"
    );

    let get_resp = crate::client::send_with_retry(|| client.get(&get_url).bearer_auth(&token))
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch message: {e}")))?;

    if !get_resp.status().is_success() {
        let err = get_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: 0,
            message: err,
            reason: "fetch_failed".to_string(),
            enable_url: None,
        });
    }

    let msg_json: Value = get_resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse message: {e}")))?;

    let thread_id = msg_json
        .get("threadId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GwsError::Other(anyhow::anyhow!("Message has no threadId")))?;

    let headers = msg_json
        .get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array());

    let mut original_from = String::new();
    let mut original_subject = String::new();
    let mut original_message_id_header = String::new();
    let mut original_references = String::new();
    let mut original_to = String::new();
    let mut original_cc = String::new();
    let mut original_date = String::new();

    if let Some(headers) = headers {
        for h in headers {
            let name = h.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
            match name {
                "From" => original_from = value.to_string(),
                "To" => original_to = value.to_string(),
                "Cc" => original_cc = value.to_string(),
                "Subject" => original_subject = value.to_string(),
                "Message-ID" | "Message-Id" => {
                    original_message_id_header = value.to_string();
                }
                "References" => original_references = value.to_string(),
                "Date" => original_date = value.to_string(),
                _ => {}
            }
        }
    }

    // Build reply subject
    let subject = if original_subject.to_lowercase().starts_with("re:") {
        original_subject.clone()
    } else {
        format!("Re: {original_subject}")
    };

    // Build recipients
    let reply_to = if reply_all {
        // Reply-all: original From + original To + original Cc, minus self
        let mut recipients = vec![original_from.clone()];
        if !original_to.is_empty() {
            recipients.push(original_to.clone());
        }
        if !original_cc.is_empty() {
            recipients.push(original_cc.clone());
        }
        recipients.join(", ")
    } else {
        original_from.clone()
    };

    // Build References header (append original Message-ID to existing References)
    let references = if original_references.is_empty() {
        original_message_id_header.clone()
    } else {
        format!("{original_references} {original_message_id_header}")
    };

    // Extract plain text body from original for quoting
    let original_body = extract_plain_text(&msg_json);
    let quoted = quote_body(&original_body, &original_from, &original_date);

    // Build raw RFC 2822 message
    let raw_message = build_reply_message(
        &reply_to,
        &subject,
        body_text,
        &quoted,
        &original_message_id_header,
        &references,
    );

    let encoded = URL_SAFE.encode(&raw_message);
    let send_body = json!({
        "raw": encoded,
        "threadId": thread_id,
    });

    // Send via Gmail API
    let send_url = "https://gmail.googleapis.com/gmail/v1/users/me/messages/send";
    let send_resp = crate::client::send_with_retry(|| {
        client
            .post(send_url)
            .bearer_auth(&token)
            .json(&send_body)
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to send reply: {e}")))?;

    if !send_resp.status().is_success() {
        let err = send_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: 0,
            message: err,
            reason: "send_failed".to_string(),
            enable_url: None,
        });
    }

    let resp_json: Value = send_resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse send response: {e}")))?;

    let output = json!({
        "status": "sent",
        "id": resp_json.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "threadId": resp_json.get("threadId").and_then(|v| v.as_str()).unwrap_or(""),
    });

    println!(
        "{}",
        crate::formatter::format_value(&output, &crate::formatter::OutputFormat::default())
    );

    Ok(())
}

/// Extract plain text body from a Gmail message JSON.
pub(super) fn extract_plain_text(msg: &Value) -> String {
    // Try to find a text/plain part in the payload
    if let Some(payload) = msg.get("payload") {
        if let Some(body_text) = extract_text_from_part(payload) {
            return body_text;
        }
    }
    String::new()
}

/// Recursively extract text/plain content from a MIME part.
fn extract_text_from_part(part: &Value) -> Option<String> {
    let mime_type = part.get("mimeType").and_then(|v| v.as_str()).unwrap_or("");

    if mime_type == "text/plain" {
        if let Some(data) = part
            .get("body")
            .and_then(|b| b.get("data"))
            .and_then(|d| d.as_str())
        {
            if let Ok(decoded) = URL_SAFE.decode(data) {
                return String::from_utf8(decoded).ok();
            }
        }
    }

    // Check nested parts (multipart messages)
    if let Some(parts) = part.get("parts").and_then(|p| p.as_array()) {
        for sub_part in parts {
            if let Some(text) = extract_text_from_part(sub_part) {
                return Some(text);
            }
        }
    }

    None
}

/// Format original message body as a quote block.
fn quote_body(body: &str, from: &str, date: &str) -> String {
    if body.is_empty() {
        return String::new();
    }

    let quoted_lines: Vec<String> = body.lines().map(|l| format!("> {l}")).collect();
    format!(
        "\r\n\r\nOn {date}, {from} wrote:\r\n{}",
        quoted_lines.join("\r\n")
    )
}

/// Build an RFC 2822 reply message with proper threading headers.
fn build_reply_message(
    to: &str,
    subject: &str,
    body: &str,
    quoted: &str,
    in_reply_to: &str,
    references: &str,
) -> String {
    let mut msg = format!("To: {to}\r\nSubject: {subject}\r\n");

    if !in_reply_to.is_empty() {
        msg.push_str(&format!("In-Reply-To: {in_reply_to}\r\n"));
    }
    if !references.is_empty() {
        msg.push_str(&format!("References: {references}\r\n"));
    }

    msg.push_str(&format!("\r\n{body}{quoted}"));
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_body() {
        let quoted = quote_body("Hello\nWorld", "alice@example.com", "Mon, 3 Mar 2026");
        assert!(quoted.contains("> Hello"));
        assert!(quoted.contains("> World"));
        assert!(quoted.contains("alice@example.com wrote:"));
    }

    #[test]
    fn test_quote_body_empty() {
        let quoted = quote_body("", "alice@example.com", "Mon, 3 Mar 2026");
        assert!(quoted.is_empty());
    }

    #[test]
    fn test_build_reply_message() {
        let msg = build_reply_message(
            "alice@example.com",
            "Re: Hello",
            "Thanks!",
            "\r\n\r\nOn Mon, alice@example.com wrote:\r\n> Hi",
            "<abc@mail.gmail.com>",
            "<abc@mail.gmail.com>",
        );
        assert!(msg.contains("To: alice@example.com"));
        assert!(msg.contains("Subject: Re: Hello"));
        assert!(msg.contains("In-Reply-To: <abc@mail.gmail.com>"));
        assert!(msg.contains("References: <abc@mail.gmail.com>"));
        assert!(msg.contains("Thanks!"));
        assert!(msg.contains("> Hi"));
    }

    #[test]
    fn test_build_reply_message_no_headers() {
        let msg = build_reply_message("bob@example.com", "Re: Test", "Got it", "", "", "");
        assert!(msg.contains("To: bob@example.com"));
        assert!(!msg.contains("In-Reply-To:"));
        assert!(!msg.contains("References:"));
    }

    #[test]
    fn test_extract_text_from_part_plain() {
        let encoded = URL_SAFE.encode("Hello World");
        let part = json!({
            "mimeType": "text/plain",
            "body": {
                "data": encoded,
            },
        });
        let text = extract_text_from_part(&part);
        assert_eq!(text, Some("Hello World".to_string()));
    }

    #[test]
    fn test_extract_text_from_part_multipart() {
        let encoded = URL_SAFE.encode("Nested text");
        let part = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {
                    "mimeType": "text/plain",
                    "body": {
                        "data": encoded,
                    },
                },
                {
                    "mimeType": "text/html",
                    "body": {
                        "data": URL_SAFE.encode("<p>HTML</p>"),
                    },
                },
            ],
        });
        let text = extract_text_from_part(&part);
        assert_eq!(text, Some("Nested text".to_string()));
    }
}

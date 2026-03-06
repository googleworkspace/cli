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

//! Gmail `+forward` helper — forward an existing message to new recipients.

use super::*;

/// Handle the `+forward` subcommand.
pub async fn handle_forward(matches: &ArgMatches) -> Result<(), GwsError> {
    let message_id = matches.get_one::<String>("message").unwrap();
    let to = matches.get_one::<String>("to").unwrap();
    let body_text = matches
        .get_one::<String>("body")
        .map(|s| s.as_str())
        .unwrap_or("");

    // Authenticate
    let token = auth::get_token(&[GMAIL_SCOPE], None)
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // 1. Fetch original message (full format to get body + attachments)
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

    let headers = msg_json
        .get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array());

    let mut original_from = String::new();
    let mut original_subject = String::new();
    let mut original_date = String::new();
    let mut original_to = String::new();

    if let Some(headers) = headers {
        for h in headers {
            let name = h.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
            match name {
                "From" => original_from = value.to_string(),
                "To" => original_to = value.to_string(),
                "Subject" => original_subject = value.to_string(),
                "Date" => original_date = value.to_string(),
                _ => {}
            }
        }
    }

    // Build forwarded subject
    let subject = if original_subject.to_lowercase().starts_with("fwd:") {
        original_subject.clone()
    } else {
        format!("Fwd: {original_subject}")
    };

    // Extract plain text body from original
    let original_body = super::reply::extract_plain_text(&msg_json);

    // Build forwarded message header
    let forwarded_header = build_forwarded_header(
        &original_from,
        &original_date,
        &original_subject,
        &original_to,
    );

    // Collect attachment metadata from the original message
    let attachments = collect_attachments(&msg_json);

    if attachments.is_empty() {
        // Simple forward without attachments — use raw message
        let raw_message = build_forward_message(
            to,
            &subject,
            body_text,
            &forwarded_header,
            &original_body,
        );

        let encoded = URL_SAFE.encode(&raw_message);
        let send_body = json!({
            "raw": encoded,
        });

        let send_url = "https://gmail.googleapis.com/gmail/v1/users/me/messages/send";
        let send_resp = crate::client::send_with_retry(|| {
            client
                .post(send_url)
                .bearer_auth(&token)
                .json(&send_body)
        })
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to send forward: {e}")))?;

        if !send_resp.status().is_success() {
            let err = send_resp.text().await.unwrap_or_default();
            return Err(GwsError::Api {
                code: 0,
                message: err,
                reason: "send_failed".to_string(),
                enable_url: None,
            });
        }

        let resp_json: Value = send_resp.json().await.map_err(|e| {
            GwsError::Other(anyhow::anyhow!("Failed to parse send response: {e}"))
        })?;

        print_result(&resp_json);
    } else {
        // Forward with attachments — build MIME multipart message
        let mime_message = build_multipart_forward(
            to,
            &subject,
            body_text,
            &forwarded_header,
            &original_body,
            &attachments,
            message_id,
            &token,
            &client,
        )
        .await?;

        let encoded = URL_SAFE.encode(&mime_message);
        let send_body = json!({
            "raw": encoded,
        });

        let send_url = "https://gmail.googleapis.com/gmail/v1/users/me/messages/send";
        let send_resp = crate::client::send_with_retry(|| {
            client
                .post(send_url)
                .bearer_auth(&token)
                .json(&send_body)
        })
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to send forward: {e}")))?;

        if !send_resp.status().is_success() {
            let err = send_resp.text().await.unwrap_or_default();
            return Err(GwsError::Api {
                code: 0,
                message: err,
                reason: "send_failed".to_string(),
                enable_url: None,
            });
        }

        let resp_json: Value = send_resp.json().await.map_err(|e| {
            GwsError::Other(anyhow::anyhow!("Failed to parse send response: {e}"))
        })?;

        print_result(&resp_json);
    }

    Ok(())
}

fn print_result(resp_json: &Value) {
    let output = json!({
        "status": "sent",
        "id": resp_json.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "threadId": resp_json.get("threadId").and_then(|v| v.as_str()).unwrap_or(""),
    });

    println!(
        "{}",
        crate::formatter::format_value(&output, &crate::formatter::OutputFormat::default())
    );
}

/// Metadata for an attachment in the original message.
struct AttachmentInfo {
    attachment_id: String,
    filename: String,
    mime_type: String,
}

/// Collect attachment info from a Gmail message payload.
fn collect_attachments(msg: &Value) -> Vec<AttachmentInfo> {
    let mut attachments = Vec::new();
    if let Some(payload) = msg.get("payload") {
        collect_attachments_from_part(payload, &mut attachments);
    }
    attachments
}

/// Recursively scan MIME parts for attachments.
fn collect_attachments_from_part(part: &Value, attachments: &mut Vec<AttachmentInfo>) {
    let filename = part
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !filename.is_empty() {
        if let Some(attachment_id) = part
            .get("body")
            .and_then(|b| b.get("attachmentId"))
            .and_then(|v| v.as_str())
        {
            let mime_type = part
                .get("mimeType")
                .and_then(|v| v.as_str())
                .unwrap_or("application/octet-stream");

            attachments.push(AttachmentInfo {
                attachment_id: attachment_id.to_string(),
                filename: filename.to_string(),
                mime_type: mime_type.to_string(),
            });
        }
    }

    // Recurse into sub-parts
    if let Some(parts) = part.get("parts").and_then(|p| p.as_array()) {
        for sub_part in parts {
            collect_attachments_from_part(sub_part, attachments);
        }
    }
}

/// Build a simple forwarded message (no attachments).
fn build_forward_message(
    to: &str,
    subject: &str,
    body: &str,
    forwarded_header: &str,
    original_body: &str,
) -> String {
    format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{body}\r\n\r\n{forwarded_header}\r\n{original_body}"
    )
}

/// Build a MIME multipart message with attachments.
async fn build_multipart_forward(
    to: &str,
    subject: &str,
    body: &str,
    forwarded_header: &str,
    original_body: &str,
    attachments: &[AttachmentInfo],
    message_id: &str,
    token: &str,
    client: &reqwest::Client,
) -> Result<String, GwsError> {
    use base64::engine::general_purpose::STANDARD;

    let boundary = format!("boundary_{}", generate_boundary_id());

    let mut msg = String::new();
    msg.push_str(&format!("To: {to}\r\n"));
    msg.push_str(&format!("Subject: {subject}\r\n"));
    msg.push_str(&format!(
        "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n"
    ));
    msg.push_str("\r\n");

    // Text body part
    msg.push_str(&format!("--{boundary}\r\n"));
    msg.push_str("Content-Type: text/plain; charset=UTF-8\r\n\r\n");
    msg.push_str(body);
    msg.push_str("\r\n\r\n");
    msg.push_str(forwarded_header);
    msg.push_str("\r\n");
    msg.push_str(original_body);
    msg.push_str("\r\n");

    // Attachment parts
    let encoded_msg_id = crate::validate::encode_path_segment(message_id);
    for att in attachments {
        let encoded_att_id = crate::validate::encode_path_segment(&att.attachment_id);
        let att_url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{encoded_msg_id}/attachments/{encoded_att_id}"
        );

        let att_resp = crate::client::send_with_retry(|| client.get(&att_url).bearer_auth(token))
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch attachment: {e}")))?;

        if !att_resp.status().is_success() {
            let err = att_resp.text().await.unwrap_or_default();
            return Err(GwsError::Api {
                code: 0,
                message: err,
                reason: "attachment_fetch_failed".to_string(),
                enable_url: None,
            });
        }

        let att_json: Value = att_resp.json().await.map_err(|e| {
            GwsError::Other(anyhow::anyhow!("Failed to parse attachment response: {e}"))
        })?;

        // Gmail returns attachment data in URL-safe base64; re-encode as standard base64
        // for the MIME message.
        let url_safe_data = att_json
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let raw_bytes = URL_SAFE
            .decode(url_safe_data)
            .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to decode attachment: {e}")))?;

        let standard_b64 = STANDARD.encode(&raw_bytes);

        msg.push_str(&format!("--{boundary}\r\n"));
        msg.push_str(&format!(
            "Content-Type: {}; name=\"{}\"\r\n",
            att.mime_type, att.filename
        ));
        msg.push_str("Content-Transfer-Encoding: base64\r\n");
        msg.push_str(&format!(
            "Content-Disposition: attachment; filename=\"{}\"\r\n",
            att.filename
        ));
        msg.push_str("\r\n");

        // Wrap base64 at 76 chars per RFC 2045
        for chunk in standard_b64.as_bytes().chunks(76) {
            msg.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            msg.push_str("\r\n");
        }
    }

    msg.push_str(&format!("--{boundary}--\r\n"));

    Ok(msg)
}

/// Build the forwarded message attribution header.
fn build_forwarded_header(from: &str, date: &str, subject: &str, to: &str) -> String {
    let mut header = String::from("---------- Forwarded message ---------");
    header.push_str(&format!("\r\nFrom: {from}"));
    header.push_str(&format!("\r\nDate: {date}"));
    header.push_str(&format!("\r\nSubject: {subject}"));
    header.push_str(&format!("\r\nTo: {to}"));
    header
}

/// Generate a simple identifier for MIME boundaries.
fn generate_boundary_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_forwarded_header() {
        let header = build_forwarded_header(
            "alice@example.com",
            "Mon, 3 Mar 2026",
            "Hello",
            "bob@example.com",
        );
        assert!(header.contains("---------- Forwarded message ---------"));
        assert!(header.contains("From: alice@example.com"));
        assert!(header.contains("Date: Mon, 3 Mar 2026"));
        assert!(header.contains("Subject: Hello"));
        assert!(header.contains("To: bob@example.com"));
    }

    #[test]
    fn test_build_forward_message() {
        let msg = build_forward_message(
            "bob@example.com",
            "Fwd: Hello",
            "FYI",
            "---------- Forwarded message ---------\r\nFrom: alice@example.com",
            "Original body text",
        );
        assert!(msg.contains("To: bob@example.com"));
        assert!(msg.contains("Subject: Fwd: Hello"));
        assert!(msg.contains("FYI"));
        assert!(msg.contains("---------- Forwarded message ---------"));
        assert!(msg.contains("Original body text"));
    }

    #[test]
    fn test_collect_attachments_empty() {
        let msg = json!({
            "payload": {
                "mimeType": "text/plain",
                "body": { "data": "dGVzdA==" },
            },
        });
        let attachments = collect_attachments(&msg);
        assert!(attachments.is_empty());
    }

    #[test]
    fn test_collect_attachments_with_attachment() {
        let msg = json!({
            "payload": {
                "mimeType": "multipart/mixed",
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": { "data": "dGVzdA==" },
                        "filename": "",
                    },
                    {
                        "mimeType": "application/pdf",
                        "filename": "report.pdf",
                        "body": {
                            "attachmentId": "ATT_123",
                            "size": 1024,
                        },
                    },
                ],
            },
        });
        let attachments = collect_attachments(&msg);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "report.pdf");
        assert_eq!(attachments[0].attachment_id, "ATT_123");
        assert_eq!(attachments[0].mime_type, "application/pdf");
    }

    #[test]
    fn test_generate_boundary_id() {
        let id = generate_boundary_id();
        assert!(!id.is_empty());
        // Should be a hex string
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

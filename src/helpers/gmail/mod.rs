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
pub mod forward;
pub mod reply;
pub mod send;
pub mod triage;
pub mod watch;

use forward::handle_forward;
use reply::handle_reply;
use send::handle_send;
use triage::handle_triage;
use watch::handle_watch;

pub(super) use crate::auth;
pub(super) use crate::error::GwsError;
pub(super) use crate::executor;
pub(super) use anyhow::Context;
pub(super) use base64::{engine::general_purpose::URL_SAFE, Engine as _};
pub(super) use clap::{Arg, ArgAction, ArgMatches, Command};
pub(super) use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct GmailHelper;

pub(super) const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";
pub(super) const GMAIL_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
pub(super) const PUBSUB_SCOPE: &str = "https://www.googleapis.com/auth/pubsub";

pub(super) struct OriginalMessage {
    pub thread_id: String,
    pub message_id_header: String,
    pub references: String,
    pub from: String,
    pub reply_to: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date: String,
    pub body_text: String,
}

impl OriginalMessage {
    /// Placeholder used for `--dry-run` to avoid requiring auth/network.
    pub(super) fn dry_run_placeholder(message_id: &str) -> Self {
        Self {
            thread_id: format!("thread-{message_id}"),
            message_id_header: format!("<{message_id}@example.com>"),
            references: String::new(),
            from: "sender@example.com".to_string(),
            reply_to: String::new(),
            to: "you@example.com".to_string(),
            cc: String::new(),
            subject: "Original subject".to_string(),
            date: "Thu, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Original message body".to_string(),
        }
    }
}

#[derive(Default)]
struct ParsedMessageHeaders {
    from: String,
    reply_to: String,
    to: String,
    cc: String,
    subject: String,
    date: String,
    message_id_header: String,
    references: String,
}

fn append_header_value(existing: &mut String, value: &str) {
    if !existing.is_empty() {
        existing.push(' ');
    }
    existing.push_str(value);
}

fn append_address_list_header_value(existing: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }

    if !existing.is_empty() {
        existing.push_str(", ");
    }
    existing.push_str(value);
}

fn parse_message_headers(headers: &[Value]) -> ParsedMessageHeaders {
    let mut parsed = ParsedMessageHeaders::default();

    for header in headers {
        let name = header.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = header.get("value").and_then(|v| v.as_str()).unwrap_or("");

        match name {
            "From" => parsed.from = value.to_string(),
            "Reply-To" => append_address_list_header_value(&mut parsed.reply_to, value),
            "To" => append_address_list_header_value(&mut parsed.to, value),
            "Cc" => append_address_list_header_value(&mut parsed.cc, value),
            "Subject" => parsed.subject = value.to_string(),
            "Date" => parsed.date = value.to_string(),
            "Message-ID" | "Message-Id" => parsed.message_id_header = value.to_string(),
            "References" => append_header_value(&mut parsed.references, value),
            _ => {}
        }
    }

    parsed
}

fn parse_original_message(msg: &Value) -> OriginalMessage {
    let thread_id = msg
        .get("threadId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let snippet = msg
        .get("snippet")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let parsed_headers = msg
        .get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array())
        .map(|headers| parse_message_headers(headers))
        .unwrap_or_default();

    let body_text = msg
        .get("payload")
        .and_then(extract_plain_text_body)
        .unwrap_or(snippet);

    OriginalMessage {
        thread_id,
        message_id_header: parsed_headers.message_id_header,
        references: parsed_headers.references,
        from: parsed_headers.from,
        reply_to: parsed_headers.reply_to,
        to: parsed_headers.to,
        cc: parsed_headers.cc,
        subject: parsed_headers.subject,
        date: parsed_headers.date,
        body_text,
    }
}

pub(super) async fn fetch_message_metadata(
    client: &reqwest::Client,
    token: &str,
    message_id: &str,
) -> Result<OriginalMessage, GwsError> {
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}",
        crate::validate::encode_path_segment(message_id)
    );

    let resp = crate::client::send_with_retry(|| {
        client
            .get(&url)
            .bearer_auth(token)
            .query(&[("format", "full")])
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch message: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let err = resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status,
            message: format!("Failed to fetch message {message_id}: {err}"),
            reason: "fetchFailed".to_string(),
            enable_url: None,
        });
    }

    let msg: Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse message: {e}")))?;

    Ok(parse_original_message(&msg))
}

fn extract_plain_text_body(payload: &Value) -> Option<String> {
    let mime_type = payload
        .get("mimeType")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if mime_type == "text/plain" {
        if let Some(data) = payload
            .get("body")
            .and_then(|b| b.get("data"))
            .and_then(|d| d.as_str())
        {
            if let Ok(decoded) = URL_SAFE.decode(data) {
                return String::from_utf8(decoded).ok();
            }
        }
        return None;
    }

    if let Some(parts) = payload.get("parts").and_then(|p| p.as_array()) {
        for part in parts {
            if let Some(text) = extract_plain_text_body(part) {
                return Some(text);
            }
        }
    }

    None
}

/// Strip CR and LF characters to prevent header injection attacks.
pub(super) fn sanitize_header_value(value: &str) -> String {
    value.replace(['\r', '\n'], "")
}

/// RFC 2047 encode a header value if it contains non-ASCII characters.
/// Uses standard Base64 (RFC 2045) and folds at 75-char encoded-word limit.
pub(super) fn encode_header_value(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }

    use base64::engine::general_purpose::STANDARD;

    // RFC 2047 specifies a 75-character limit for encoded-words.
    // Max raw length of 45 bytes -> 60 encoded chars. 60 + len("=?UTF-8?B??=") = 72, < 75.
    const MAX_RAW_LEN: usize = 45;

    // Chunk at character boundaries to avoid splitting multi-byte UTF-8 sequences.
    let mut chunks: Vec<&str> = Vec::new();
    let mut start = 0;
    for (i, ch) in value.char_indices() {
        if i + ch.len_utf8() - start > MAX_RAW_LEN && i > start {
            chunks.push(&value[start..i]);
            start = i;
        }
    }
    if start < value.len() {
        chunks.push(&value[start..]);
    }

    let encoded_words: Vec<String> = chunks
        .iter()
        .map(|chunk| format!("=?UTF-8?B?{}?=", STANDARD.encode(chunk.as_bytes())))
        .collect();

    // Join with CRLF and a space for folding.
    encoded_words.join("\r\n ")
}

/// In-Reply-To and References values for threading a reply or forward.
pub(super) struct ThreadingHeaders<'a> {
    pub in_reply_to: &'a str,
    pub references: &'a str,
}

/// Shared builder for RFC 2822 email messages.
///
/// Handles header construction with CRLF sanitization and RFC 2047
/// encoding of non-ASCII subjects. Each helper owns its body assembly
/// (quoted reply, forwarded block, plain body) and passes it to `build()`.
pub(super) struct MessageBuilder<'a> {
    pub to: &'a str,
    pub subject: &'a str,
    pub from: Option<&'a str>,
    pub cc: Option<&'a str>,
    pub bcc: Option<&'a str>,
    pub threading: Option<ThreadingHeaders<'a>>,
}

impl MessageBuilder<'_> {
    /// Build the complete RFC 2822 message (headers + blank line + body).
    pub fn build(&self, body: &str) -> String {
        debug_assert!(
            !self.to.is_empty(),
            "MessageBuilder: `to` must not be empty"
        );

        let mut headers = format!(
            "To: {}\r\nSubject: {}",
            sanitize_header_value(self.to),
            // Sanitize first: stripping CRLF before encoding prevents injection
            // in encoded-words.
            encode_header_value(&sanitize_header_value(self.subject)),
        );

        if let Some(ref threading) = self.threading {
            headers.push_str(&format!(
                "\r\nIn-Reply-To: {}\r\nReferences: {}",
                sanitize_header_value(threading.in_reply_to),
                sanitize_header_value(threading.references),
            ));
        }

        headers.push_str("\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8");

        if let Some(from) = self.from {
            headers.push_str(&format!("\r\nFrom: {}", sanitize_header_value(from)));
        }

        if let Some(cc) = self.cc {
            headers.push_str(&format!("\r\nCc: {}", sanitize_header_value(cc)));
        }

        // The Gmail API reads the Bcc header to route to those recipients,
        // then strips it before delivery.
        if let Some(bcc) = self.bcc {
            headers.push_str(&format!("\r\nBcc: {}", sanitize_header_value(bcc)));
        }

        format!("{}\r\n\r\n{}", headers, body)
    }
}

/// Build the References header value. Returns just the message ID when there
/// are no prior references, or appends it to the existing chain.
pub(super) fn build_references(original_references: &str, original_message_id: &str) -> String {
    if original_references.is_empty() {
        original_message_id.to_string()
    } else {
        format!("{} {}", original_references, original_message_id)
    }
}

/// Parse an optional clap argument, trimming whitespace and treating
/// empty/whitespace-only values as None.
pub(super) fn parse_optional_trimmed(matches: &ArgMatches, name: &str) -> Option<String> {
    matches
        .get_one::<String>(name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub(super) fn resolve_send_method(
    doc: &crate::discovery::RestDescription,
) -> Result<&crate::discovery::RestMethod, GwsError> {
    let users_res = doc
        .resources
        .get("users")
        .ok_or_else(|| GwsError::Discovery("Resource 'users' not found".to_string()))?;
    let messages_res = users_res
        .resources
        .get("messages")
        .ok_or_else(|| GwsError::Discovery("Resource 'users.messages' not found".to_string()))?;
    messages_res
        .methods
        .get("send")
        .ok_or_else(|| GwsError::Discovery("Method 'users.messages.send' not found".to_string()))
}

/// Build the JSON request body for `users.messages.send`, base64-encoding
/// the raw RFC 2822 message and optionally including a threadId.
pub(super) fn build_raw_send_body(raw_message: &str, thread_id: Option<&str>) -> Value {
    let mut body =
        serde_json::Map::from_iter([("raw".to_string(), json!(URL_SAFE.encode(raw_message)))]);

    if let Some(thread_id) = thread_id {
        body.insert("threadId".to_string(), json!(thread_id));
    }

    Value::Object(body)
}

pub(super) async fn send_raw_email(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
    raw_message: &str,
    thread_id: Option<&str>,
    existing_token: Option<&str>,
) -> Result<(), GwsError> {
    let body = build_raw_send_body(raw_message, thread_id);
    let body_str = body.to_string();

    let send_method = resolve_send_method(doc)?;
    let params = json!({ "userId": "me" });
    let params_str = params.to_string();

    let (token, auth_method) = match existing_token {
        Some(t) => (Some(t.to_string()), executor::AuthMethod::OAuth),
        None => {
            let scopes: Vec<&str> = send_method.scopes.iter().map(|s| s.as_str()).collect();
            match auth::get_token(&scopes).await {
                Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                Err(_) if matches.get_flag("dry-run") => (None, executor::AuthMethod::None),
                Err(e) => return Err(GwsError::Auth(format!("Gmail auth failed: {e}"))),
            }
        }
    };

    let pagination = executor::PaginationConfig {
        page_all: false,
        page_limit: 10,
        page_delay_ms: 100,
    };

    executor::execute_method(
        doc,
        send_method,
        Some(&params_str),
        Some(&body_str),
        token.as_deref(),
        auth_method,
        None,
        None,
        matches.get_flag("dry-run"),
        &pagination,
        None,
        &crate::helpers::modelarmor::SanitizeMode::Warn,
        &crate::formatter::OutputFormat::default(),
        false,
    )
    .await?;

    Ok(())
}

impl Helper for GmailHelper {
    /// Injects helper subcommands (`+send`, `+watch`) into the main CLI command.
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+send")
                .about("[Helper] Send an email")
                .arg(
                    Arg::new("to")
                        .long("to")
                        .help("Recipient email address(es), comma-separated")
                        .required(true)
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("subject")
                        .long("subject")
                        .help("Email subject")
                        .required(true)
                        .value_name("SUBJECT"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .help("Email body (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("cc")
                        .long("cc")
                        .help("CC email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("bcc")
                        .long("bcc")
                        .help("BCC email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .help("Show the request that would be sent without executing it")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi Alice!'
  gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --cc bob@example.com
  gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --bcc secret@example.com

TIPS:
  Handles RFC 2822 formatting and base64 encoding automatically.
  For HTML bodies or attachments, use the raw API instead: gws gmail users messages send --json '...'",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+triage")
                .about("[Helper] Show unread inbox summary (sender, subject, date)")
                .arg(
                    Arg::new("max")
                        .long("max")
                        .help("Maximum messages to show (default: 20)")
                        .default_value("20")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("query")
                        .long("query")
                        .help("Gmail search query (default: is:unread)")
                        .value_name("QUERY"),
                )
                .arg(
                    Arg::new("labels")
                        .long("labels")
                        .help("Include label names in output")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +triage
  gws gmail +triage --max 5 --query 'from:boss'
  gws gmail +triage --format json | jq '.[].subject'
  gws gmail +triage --labels

TIPS:
  Read-only — never modifies your mailbox.
  Defaults to table output format.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+reply")
                .about("[Helper] Reply to a message (handles threading automatically)")
                .arg(
                    Arg::new("message-id")
                        .long("message-id")
                        .help("Gmail message ID to reply to")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .help("Reply body (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("from")
                        .long("from")
                        .help("Sender address (for send-as/alias; omit to use account default)")
                        .value_name("EMAIL"),
                )
                .arg(
                    Arg::new("to")
                        .long("to")
                        .help("Additional To email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("cc")
                        .long("cc")
                        .help("Additional CC email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("bcc")
                        .long("bcc")
                        .help("BCC email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .help("Show the request that would be sent without executing it")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Thanks, got it!'
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Looping in Carol' --cc carol@example.com
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Adding Dave' --to dave@example.com
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Reply' --bcc secret@example.com

TIPS:
  Automatically sets In-Reply-To, References, and threadId headers.
  Quotes the original message in the reply body.
  --to adds extra recipients to the To field.
  For reply-all, use +reply-all instead.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+reply-all")
                .about("[Helper] Reply-all to a message (handles threading automatically)")
                .arg(
                    Arg::new("message-id")
                        .long("message-id")
                        .help("Gmail message ID to reply to")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .help("Reply body (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("from")
                        .long("from")
                        .help("Sender address (for send-as/alias; omit to use account default)")
                        .value_name("EMAIL"),
                )
                .arg(
                    Arg::new("to")
                        .long("to")
                        .help("Additional To email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("cc")
                        .long("cc")
                        .help("Additional CC email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("bcc")
                        .long("bcc")
                        .help("BCC email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("remove")
                        .long("remove")
                        .help("Exclude recipients from the outgoing reply (comma-separated emails)")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .help("Show the request that would be sent without executing it")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Sounds good to me!'
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Updated' --remove bob@example.com
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Adding Eve' --cc eve@example.com
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Adding Dave' --to dave@example.com
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Reply' --bcc secret@example.com

TIPS:
  Replies to the sender and all original To/CC recipients.
  Use --to to add extra recipients to the To field.
  Use --cc to add new CC recipients.
  Use --bcc for recipients who should not be visible to others.
  Use --remove to exclude recipients from the outgoing reply, including the sender or Reply-To target.
  The command fails if no To recipient remains after exclusions and --to additions.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+forward")
                .about("[Helper] Forward a message to new recipients")
                .arg(
                    Arg::new("message-id")
                        .long("message-id")
                        .help("Gmail message ID to forward")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("to")
                        .long("to")
                        .help("Recipient email address(es), comma-separated")
                        .required(true)
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("from")
                        .long("from")
                        .help("Sender address (for send-as/alias; omit to use account default)")
                        .value_name("EMAIL"),
                )
                .arg(
                    Arg::new("cc")
                        .long("cc")
                        .help("CC email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("bcc")
                        .long("bcc")
                        .help("BCC email address(es), comma-separated")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .help("Optional note to include above the forwarded message")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .help("Show the request that would be sent without executing it")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --body 'FYI see below'
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --cc eve@example.com
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --bcc secret@example.com

TIPS:
  Includes the original message with sender, date, subject, and recipients.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+watch")
                .about("[Helper] Watch for new emails and stream them as NDJSON")
                .arg(
                    Arg::new("project")
                        .long("project")
                        .help("GCP project ID for Pub/Sub resources")
                        .value_name("PROJECT"),
                )
                .arg(
                    Arg::new("subscription")
                        .long("subscription")
                        .help("Existing Pub/Sub subscription name (skip setup)")
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("topic")
                        .long("topic")
                        .help("Existing Pub/Sub topic with Gmail push permission already granted")
                        .value_name("TOPIC"),
                )
                .arg(
                    Arg::new("label-ids")
                        .long("label-ids")
                        .help("Comma-separated Gmail label IDs to filter (e.g., INBOX,UNREAD)")
                        .value_name("LABELS"),
                )
                .arg(
                    Arg::new("max-messages")
                        .long("max-messages")
                        .help("Max messages per pull batch")
                        .value_name("N")
                        .default_value("10"),
                )
                .arg(
                    Arg::new("poll-interval")
                        .long("poll-interval")
                        .help("Seconds between pulls")
                        .value_name("SECS")
                        .default_value("5"),
                )
                .arg(
                    Arg::new("msg-format")
                        .long("msg-format")
                        .help("Gmail message format: full, metadata, minimal, raw")
                        .value_name("FORMAT")
                        .value_parser(["full", "metadata", "minimal", "raw"])
                        .default_value("full"),
                )
                .arg(
                    Arg::new("once")
                        .long("once")
                        .help("Pull once and exit")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("cleanup")
                        .long("cleanup")
                        .help("Delete created Pub/Sub resources on exit")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("output-dir")
                        .long("output-dir")
                        .help("Write each message to a separate JSON file in this directory")
                        .value_name("DIR"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +watch --project my-gcp-project
  gws gmail +watch --project my-project --label-ids INBOX --once
  gws gmail +watch --subscription projects/p/subscriptions/my-sub
  gws gmail +watch --project my-project --cleanup --output-dir ./emails

TIPS:
  Gmail watch expires after 7 days — re-run to renew.
  Without --cleanup, Pub/Sub resources persist for reconnection.
  Press Ctrl-C to stop gracefully.",
                ),
        );

        cmd
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(matches) = matches.subcommand_matches("+send") {
                handle_send(doc, matches).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+reply") {
                handle_reply(doc, matches, false).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+reply-all") {
                handle_reply(doc, matches, true).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+forward") {
                handle_forward(doc, matches).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+triage") {
                handle_triage(matches).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+watch") {
                handle_watch(matches, sanitize_config).await?;
                return Ok(true);
            }

            Ok(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_inject_commands() {
        let helper = GmailHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();

        // No scopes granted -> defaults to showing all
        let cmd = helper.inject_commands(cmd, &doc);
        let subcommands: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subcommands.contains(&"+watch"));
        assert!(subcommands.contains(&"+send"));
        assert!(subcommands.contains(&"+reply"));
        assert!(subcommands.contains(&"+reply-all"));
        assert!(subcommands.contains(&"+forward"));
    }

    #[test]
    fn test_build_raw_send_body_with_thread_id() {
        let body = build_raw_send_body("raw message", Some("thread-123"));

        assert_eq!(body["raw"], URL_SAFE.encode("raw message"));
        assert_eq!(body["threadId"], "thread-123");
    }

    #[test]
    fn test_build_raw_send_body_without_thread_id() {
        let body = build_raw_send_body("raw message", None);

        assert_eq!(body["raw"], URL_SAFE.encode("raw message"));
        assert!(body.get("threadId").is_none());
    }

    #[test]
    fn test_append_address_list_header_value() {
        let mut header_value = String::new();

        append_address_list_header_value(&mut header_value, "alice@example.com");
        append_address_list_header_value(&mut header_value, "bob@example.com");
        append_address_list_header_value(&mut header_value, "");

        assert_eq!(header_value, "alice@example.com, bob@example.com");
    }

    #[test]
    fn test_parse_original_message_concatenates_repeated_address_and_reference_headers() {
        let msg = json!({
            "threadId": "thread-123",
            "snippet": "Snippet fallback",
            "payload": {
                "mimeType": "text/html",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Reply-To", "value": "team@example.com" },
                    { "name": "Reply-To", "value": "owner@example.com" },
                    { "name": "To", "value": "bob@example.com" },
                    { "name": "To", "value": "carol@example.com" },
                    { "name": "Cc", "value": "dave@example.com" },
                    { "name": "Cc", "value": "erin@example.com" },
                    { "name": "Subject", "value": "Hello" },
                    { "name": "Date", "value": "Fri, 6 Mar 2026 12:00:00 +0000" },
                    { "name": "Message-ID", "value": "<msg@example.com>" },
                    { "name": "References", "value": "<ref-1@example.com>" },
                    { "name": "References", "value": "<ref-2@example.com>" }
                ],
                "body": {
                    "data": URL_SAFE.encode("<p>HTML only</p>")
                }
            }
        });

        let original = parse_original_message(&msg);

        assert_eq!(original.thread_id, "thread-123");
        assert_eq!(original.from, "alice@example.com");
        assert_eq!(original.reply_to, "team@example.com, owner@example.com");
        assert_eq!(original.to, "bob@example.com, carol@example.com");
        assert_eq!(original.cc, "dave@example.com, erin@example.com");
        assert_eq!(original.subject, "Hello");
        assert_eq!(original.date, "Fri, 6 Mar 2026 12:00:00 +0000");
        assert_eq!(original.message_id_header, "<msg@example.com>");
        assert_eq!(
            original.references,
            "<ref-1@example.com> <ref-2@example.com>"
        );
        assert_eq!(original.body_text, "Snippet fallback");
    }

    #[test]
    fn test_sanitize_header_value_strips_crlf() {
        assert_eq!(
            sanitize_header_value("alice@example.com\r\nBcc: evil@attacker.com"),
            "alice@example.comBcc: evil@attacker.com"
        );
        assert_eq!(sanitize_header_value("normal value"), "normal value");
        assert_eq!(sanitize_header_value("bare\nnewline"), "barenewline");
        assert_eq!(sanitize_header_value("bare\rreturn"), "barereturn");
    }

    #[test]
    fn test_encode_header_value_ascii() {
        assert_eq!(encode_header_value("Hello World"), "Hello World");
    }

    #[test]
    fn test_encode_header_value_non_ascii_short() {
        let encoded = encode_header_value("Solar — Quote");
        assert_eq!(encoded, "=?UTF-8?B?U29sYXIg4oCUIFF1b3Rl?=");
    }

    #[test]
    fn test_encode_header_value_non_ascii_long_folds() {
        let long_subject = "This is a very long subject line that contains non-ASCII characters like — and it must be folded to respect the 75-character line limit of RFC 2047.";
        let encoded = encode_header_value(long_subject);

        assert!(encoded.contains("\r\n "), "Encoded string should be folded");
        let parts: Vec<&str> = encoded.split("\r\n ").collect();
        assert!(parts.len() > 1, "Should be multiple parts");
        for part in &parts {
            assert!(part.starts_with("=?UTF-8?B?"));
            assert!(part.ends_with("?="));
            assert!(part.len() <= 75, "Part too long: {} chars", part.len());
        }
    }

    #[test]
    fn test_encode_header_value_multibyte_boundary() {
        use base64::engine::general_purpose::STANDARD;
        let subject = format!("{}€€€", "A".repeat(43));
        let encoded = encode_header_value(&subject);
        for part in encoded.split("\r\n ") {
            let b64 = part.trim_start_matches("=?UTF-8?B?").trim_end_matches("?=");
            let decoded = STANDARD.decode(b64).expect("valid base64");
            String::from_utf8(decoded).expect("each chunk must be valid UTF-8");
        }
    }

    #[test]
    fn test_message_builder_basic() {
        let raw = MessageBuilder {
            to: "test@example.com",
            subject: "Hello",
            from: None,
            cc: None,
            bcc: None,
            threading: None,
        }
        .build("World");

        assert!(raw.contains("To: test@example.com"));
        assert!(raw.contains("Subject: Hello"));
        assert!(raw.contains("MIME-Version: 1.0"));
        assert!(raw.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(raw.contains("\r\n\r\nWorld"));
        assert!(!raw.contains("From:"));
        assert!(!raw.contains("Cc:"));
        assert!(!raw.contains("Bcc:"));
        assert!(!raw.contains("In-Reply-To:"));
        assert!(!raw.contains("References:"));
    }

    #[test]
    fn test_message_builder_all_optional_headers() {
        let raw = MessageBuilder {
            to: "alice@example.com",
            subject: "Re: Hello",
            from: Some("alias@example.com"),
            cc: Some("carol@example.com"),
            bcc: Some("secret@example.com"),
            threading: Some(ThreadingHeaders {
                in_reply_to: "<abc@example.com>",
                references: "<abc@example.com>",
            }),
        }
        .build("Reply body");

        assert!(raw.contains("To: alice@example.com"));
        assert!(raw.contains("Subject: Re: Hello"));
        assert!(raw.contains("From: alias@example.com"));
        assert!(raw.contains("Cc: carol@example.com"));
        assert!(raw.contains("Bcc: secret@example.com"));
        assert!(raw.contains("In-Reply-To: <abc@example.com>"));
        assert!(raw.contains("References: <abc@example.com>"));
        assert!(raw.contains("Reply body"));
    }

    #[test]
    fn test_message_builder_non_ascii_subject() {
        let raw = MessageBuilder {
            to: "test@example.com",
            subject: "Solar — Quote Request",
            from: None,
            cc: None,
            bcc: None,
            threading: None,
        }
        .build("Body");

        assert!(raw.contains("=?UTF-8?B?"));
        assert!(!raw.contains("Solar — Quote Request"));
    }

    #[test]
    fn test_message_builder_sanitizes_crlf_injection() {
        let raw = MessageBuilder {
            to: "alice@example.com\r\nBcc: evil@attacker.com",
            subject: "Hello",
            from: None,
            cc: None,
            bcc: None,
            threading: None,
        }
        .build("Body");

        // The CRLF is stripped, preventing header injection. The "Bcc: evil..."
        // text becomes part of the To value, not a separate header.
        let header_section = raw.split("\r\n\r\n").next().unwrap();
        let header_lines: Vec<&str> = header_section.split("\r\n").collect();
        assert!(
            !header_lines.iter().any(|l| l.starts_with("Bcc:")),
            "No Bcc header should exist"
        );
    }

    #[test]
    fn test_message_builder_sanitizes_optional_headers() {
        let raw = MessageBuilder {
            to: "alice@example.com",
            subject: "Hello",
            from: Some("sender@example.com\r\nBcc: evil@attacker.com"),
            cc: Some("carol@example.com\r\nX-Injected: yes"),
            bcc: None,
            threading: None,
        }
        .build("Body");

        let header_section = raw.split("\r\n\r\n").next().unwrap();
        let header_lines: Vec<&str> = header_section.split("\r\n").collect();
        assert!(
            !header_lines.iter().any(|l| l.starts_with("X-Injected:")),
            "Injected header via Cc should not exist"
        );
        assert!(
            header_lines
                .iter()
                .filter(|l| l.starts_with("Bcc:"))
                .count()
                == 0,
            "Injected Bcc via From should not exist"
        );
    }

    #[test]
    fn test_build_references_empty() {
        assert_eq!(
            build_references("", "<msg-1@example.com>"),
            "<msg-1@example.com>"
        );
    }

    #[test]
    fn test_build_references_with_existing() {
        assert_eq!(
            build_references("<msg-0@example.com>", "<msg-1@example.com>"),
            "<msg-0@example.com> <msg-1@example.com>"
        );
    }

    #[test]
    fn test_resolve_send_method_finds_gmail_send_method() {
        let mut doc = crate::discovery::RestDescription::default();
        let send_method = crate::discovery::RestMethod {
            http_method: "POST".to_string(),
            path: "gmail/v1/users/{userId}/messages/send".to_string(),
            ..Default::default()
        };

        let mut messages = crate::discovery::RestResource::default();
        messages.methods.insert("send".to_string(), send_method);

        let mut users = crate::discovery::RestResource::default();
        users.resources.insert("messages".to_string(), messages);

        doc.resources = HashMap::from([("users".to_string(), users)]);

        let resolved = resolve_send_method(&doc).unwrap();

        assert_eq!(resolved.http_method, "POST");
        assert_eq!(resolved.path, "gmail/v1/users/{userId}/messages/send");
    }
}

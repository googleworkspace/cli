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
pub mod read;
pub mod reply;
pub mod send;
pub mod triage;
pub mod watch;

use forward::handle_forward;
use read::handle_read;
use reply::handle_reply;
use send::handle_send;
use triage::handle_triage;
use watch::handle_watch;

pub(super) use crate::auth;
use crate::error::sanitize_for_terminal;
pub(super) use crate::error::GwsError;
pub(super) use crate::executor;
pub(super) use anyhow::Context;
pub(super) use base64::{engine::general_purpose::URL_SAFE, Engine as _};
pub(super) use clap::{Arg, ArgAction, ArgMatches, Command};
pub(super) use mail_builder::headers::address::Address as MbAddress;
pub(super) use serde::Serialize;
pub(super) use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct GmailHelper;

pub(super) const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";
pub(super) const GMAIL_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
pub(super) const PUBSUB_SCOPE: &str = "https://www.googleapis.com/auth/pubsub";

/// Strip ASCII control characters (0x00–0x1F, 0x7F) from a string.
///
/// Defense-in-depth: mail-builder uses structured types for headers which
/// prevents most injection, but email addresses are written as raw bytes
/// inside angle brackets. Stripping control characters at the parse boundary
/// closes any residual CRLF/null-byte injection vectors before data reaches
/// mail-builder.
fn sanitize_control_chars(s: &str) -> String {
    s.chars().filter(|c| !c.is_ascii_control()).collect()
}

/// A parsed RFC 5322 mailbox: optional display name + email address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct Mailbox {
    pub name: Option<String>,
    pub email: String,
}

impl Mailbox {
    /// Parse a single address like `"Alice <alice@example.com>"` or `"alice@example.com"`.
    ///
    /// Intentionally total (never fails): this parses both user CLI input and
    /// Gmail API header values. API headers are already server-validated, so
    /// returning `Result` would force unnecessary error handling at every parse site.
    /// User-input validation happens at the `Config` boundary (non-empty `--to`);
    /// syntactic email validation is left to the Gmail API.
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        if let Some(start) = raw.rfind('<') {
            if let Some(end) = raw[start..].find('>') {
                let email = sanitize_control_chars(raw[start + 1..start + end].trim());
                let name_part = raw[..start].trim();
                let name = if name_part.is_empty() {
                    None
                } else {
                    // Strip surrounding quotes: "Alice Smith" → Alice Smith
                    let unquoted = name_part
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(name_part);
                    Some(sanitize_control_chars(unquoted))
                };
                return Self { name, email };
            }
        }
        Self {
            name: None,
            email: sanitize_control_chars(raw),
        }
    }

    /// Parse a comma-separated address list, respecting quoted strings.
    /// Empty-email entries (e.g. from trailing commas) are filtered out.
    pub fn parse_list(raw: &str) -> Vec<Self> {
        split_raw_mailbox_list(raw)
            .into_iter()
            .map(Mailbox::parse)
            .filter(|m| !m.email.is_empty())
            .collect()
    }

    /// Lowercase email for case-insensitive comparison.
    pub fn email_lowercase(&self) -> String {
        self.email.to_lowercase()
    }
}

/// Display format for logging and plain-text message bodies (not RFC 5322 headers).
/// Does not quote display names containing specials; mail-builder handles header serialization.
impl std::fmt::Display for Mailbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{} <{}>", name, self.email),
            None => write!(f, "{}", self.email),
        }
    }
}

/// Convert a single `Mailbox` to a `mail_builder::Address`.
pub(super) fn to_mb_address(mailbox: &Mailbox) -> MbAddress<'_> {
    MbAddress::new_address(mailbox.name.as_deref(), &mailbox.email)
}

/// Convert a slice of `Mailbox` to a `mail_builder::Address` (list).
pub(super) fn to_mb_address_list(mailboxes: &[Mailbox]) -> MbAddress<'_> {
    MbAddress::new_list(mailboxes.iter().map(to_mb_address).collect())
}

/// Strip angle brackets from a message ID: `"<abc@example.com>"` → `"abc@example.com"`.
pub(super) fn strip_angle_brackets(id: &str) -> &str {
    id.trim()
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(id.trim())
}

/// A parsed Gmail message fetched via the API, used as context for reply/forward.
///
/// `from` is always populated — `parse_original_message` returns an error when
/// `From` is missing. `body_text` always has a value — it falls back to the
/// message snippet when no `text/plain` MIME part is found. Semantically optional
/// fields (`cc`, `reply_to`, `date`, `body_html`) use `Option` so the compiler
/// enforces absence checks.
#[derive(Default, Serialize)]
pub(super) struct OriginalMessage {
    pub thread_id: Option<String>,
    /// Bare message ID (no angle brackets), e.g. `"abc@example.com"`.
    pub message_id: String,
    /// Bare message IDs (no angle brackets) forming the references chain.
    pub references: Vec<String>,
    pub from: Mailbox,
    /// Multiple Reply-To addresses are allowed per RFC 5322.
    pub reply_to: Option<Vec<Mailbox>>,
    pub to: Vec<Mailbox>,
    pub cc: Option<Vec<Mailbox>>,
    pub subject: String,
    pub date: Option<String>,
    pub body_text: String,
    pub body_html: Option<String>,
}

impl OriginalMessage {
    /// Placeholder used for `--dry-run` to avoid requiring auth/network.
    pub(super) fn dry_run_placeholder(message_id: &str) -> Self {
        Self {
            thread_id: Some(format!("thread-{message_id}")),
            message_id: format!("{message_id}@example.com"),
            from: Mailbox::parse("sender@example.com"),
            to: vec![Mailbox::parse("you@example.com")],
            subject: "Original subject".to_string(),
            date: Some("Thu, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original message body".to_string(),
            body_html: Some("<p>Original message body</p>".to_string()),
            ..Default::default()
        }
    }
}

/// Raw header values extracted from the Gmail API payload, before parsing into
/// structured types. Intermediate step: JSON headers → this → `OriginalMessage`.
#[derive(Default)]
struct ParsedMessageHeaders {
    from: String,
    reply_to: String,
    to: String,
    cc: String,
    subject: String,
    date: String,
    message_id: String,
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
            "Message-ID" | "Message-Id" => parsed.message_id = value.to_string(),
            "References" => append_header_value(&mut parsed.references, value),
            _ => {}
        }
    }

    parsed
}

/// Convert an empty string to `None`, or apply `f` to the non-empty string.
fn non_empty_then<T>(s: &str, f: impl FnOnce(&str) -> T) -> Option<T> {
    if s.is_empty() {
        None
    } else {
        Some(f(s))
    }
}

/// Convert an empty slice to `None`, non-empty to `Some(slice)`.
pub(super) fn non_empty_slice<T>(s: &[T]) -> Option<&[T]> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn parse_original_message(msg: &Value) -> Result<OriginalMessage, GwsError> {
    let thread_id = msg
        .get("threadId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

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

    if parsed_headers.from.is_empty() {
        return Err(GwsError::Other(anyhow::anyhow!(
            "Message is missing From header"
        )));
    }

    let message_id = strip_angle_brackets(&parsed_headers.message_id);
    if message_id.is_empty() {
        return Err(GwsError::Other(anyhow::anyhow!(
            "Message is missing Message-ID header"
        )));
    }

    let body_text = msg
        .get("payload")
        .and_then(extract_plain_text_body)
        .unwrap_or(snippet);

    let body_html = msg.get("payload").and_then(extract_html_body);

    // Parse references: split on whitespace and strip any angle brackets, producing bare IDs
    let references = parsed_headers
        .references
        .split_whitespace()
        .map(|id| strip_angle_brackets(id).to_string())
        .filter(|id| !id.is_empty())
        .collect();

    let reply_to = non_empty_then(&parsed_headers.reply_to, Mailbox::parse_list);
    let cc = non_empty_then(&parsed_headers.cc, Mailbox::parse_list);
    let date = Some(parsed_headers.date).filter(|s| !s.is_empty());

    Ok(OriginalMessage {
        thread_id,
        message_id: message_id.to_string(),
        references,
        from: Mailbox::parse(&parsed_headers.from),
        reply_to,
        to: Mailbox::parse_list(&parsed_headers.to),
        cc,
        subject: parsed_headers.subject,
        date,
        body_text,
        body_html,
    })
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
        let err = resp
            .text()
            .await
            .unwrap_or_else(|_| "(error body unreadable)".to_string());
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

    parse_original_message(&msg)
}

fn extract_body_by_mime(payload: &Value, target_mime: &str) -> Option<String> {
    let mime_type = payload
        .get("mimeType")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if mime_type == target_mime {
        if let Some(data) = payload
            .get("body")
            .and_then(|b| b.get("data"))
            .and_then(|d| d.as_str())
        {
            match URL_SAFE.decode(data) {
                Ok(decoded) => match String::from_utf8(decoded) {
                    Ok(s) => return Some(s),
                    Err(e) => {
                        eprintln!(
                            "Warning: {target_mime} body is not valid UTF-8: {}",
                            sanitize_for_terminal(&e.to_string())
                        );
                    }
                },
                Err(e) => {
                    eprintln!(
                        "Warning: {target_mime} body has invalid base64: {}",
                        sanitize_for_terminal(&e.to_string())
                    );
                }
            }
        }
        return None;
    }

    if let Some(parts) = payload.get("parts").and_then(|p| p.as_array()) {
        for part in parts {
            if let Some(body) = extract_body_by_mime(part, target_mime) {
                return Some(body);
            }
        }
    }

    None
}

fn extract_plain_text_body(payload: &Value) -> Option<String> {
    extract_body_by_mime(payload, "text/plain")
}

fn extract_html_body(payload: &Value) -> Option<String> {
    extract_body_by_mime(payload, "text/html")
}

/// Resolve the HTML body for quoting or forwarding: use the original HTML
/// body if available, otherwise escape the plain text and convert newlines
/// to `<br>` tags.
pub(super) fn resolve_html_body(original: &OriginalMessage) -> String {
    match &original.body_html {
        Some(html) => html.clone(),
        None => html_escape(&original.body_text)
            .lines()
            .collect::<Vec<_>>()
            .join("<br>\r\n"),
    }
}

/// Escape `&`, `<`, `>`, `"`, `'` for safe embedding in HTML.
pub(super) fn html_escape(text: &str) -> String {
    // `&` must be replaced first to avoid double-escaping the other replacements.
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Split an RFC 5322 mailbox list on commas, respecting quoted strings.
/// Returns raw string slices — use `Mailbox::parse_list` for structured parsing.
fn split_raw_mailbox_list(header: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    let mut prev_backslash = false;

    for (i, ch) in header.char_indices() {
        match ch {
            '\\' if in_quotes => {
                prev_backslash = !prev_backslash;
                continue;
            }
            '"' if !prev_backslash => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let token = header[start..i].trim();
                if !token.is_empty() {
                    result.push(token);
                }
                start = i + 1;
            }
            _ => {}
        }
        prev_backslash = false;
    }

    let token = header[start..].trim();
    if !token.is_empty() {
        result.push(token);
    }

    result
}

/// Wrap an email address in an HTML mailto link: `<a href="mailto:e">e</a>`.
///
/// The email is percent-encoded in the href to prevent mailto parameter
/// injection (e.g., `?cc=evil@example.com`) and HTML-escaped in the display text.
pub(super) fn format_email_link(email: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    let url_encoded = utf8_percent_encode(email, NON_ALPHANUMERIC);
    let display_escaped = html_escape(email);
    format!("<a href=\"mailto:{url_encoded}\">{display_escaped}</a>")
}

/// Format a `Mailbox` for the reply attribution line with a mailto link.
/// `Mailbox { name: Some("Alice"), email: "alice@example.com" }` →
/// `Alice &lt;<a href="mailto:alice%40example%2Ecom">alice@example.com</a>&gt;`
pub(super) fn format_sender_for_attribution(mailbox: &Mailbox) -> String {
    match &mailbox.name {
        Some(name) => format!(
            "{} &lt;{}&gt;",
            html_escape(name),
            format_email_link(&mailbox.email),
        ),
        None => format_email_link(&mailbox.email),
    }
}

/// Format a slice of mailboxes with mailto links on each address.
/// Used for forward To/CC fields in HTML mode.
pub(super) fn format_address_list_with_links(mailboxes: &[Mailbox]) -> String {
    mailboxes
        .iter()
        .map(format_sender_for_attribution)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reformat an RFC 2822 date to Gmail's human-friendly attribution style:
/// `"Wed, Mar 4, 2026 at 3:01\u{202f}PM"` (`\u{202f}` = narrow no-break space
/// before AM/PM). Falls back to the raw date (HTML-escaped) if chrono cannot
/// parse it.
pub(super) fn format_date_for_attribution(raw_date: &str) -> String {
    chrono::DateTime::parse_from_rfc2822(raw_date)
        .map(|dt| html_escape(&dt.format("%a, %b %-d, %Y at %-I:%M\u{202f}%p").to_string()))
        .unwrap_or_else(|e| {
            eprintln!(
                "Note: could not parse date as RFC 2822 ({}); using raw value.",
                sanitize_for_terminal(&e.to_string())
            );
            html_escape(raw_date)
        })
}

/// Format the From line for a forwarded message using Gmail's `gmail_sendername` structure.
/// When the address has a display name, it is shown in `<strong>` with the email in a mailto
/// link. Bare emails appear in both positions (matching Gmail's behavior).
pub(super) fn format_forward_from(mailbox: &Mailbox) -> String {
    let display = match &mailbox.name {
        Some(name) => name.as_str(),
        None => &mailbox.email,
    };
    format!(
        "<strong class=\"gmail_sendername\" dir=\"auto\">{}</strong> \
         <span dir=\"auto\">&lt;{}&gt;</span>",
        html_escape(display),
        format_email_link(&mailbox.email),
    )
}

/// Threading headers for reply/forward.
///
/// IDs must be bare (no angle brackets) — `set_threading_headers` passes them to
/// mail-builder which adds angle brackets per RFC 5322. `in_reply_to` is a single
/// message ID (the direct parent); `references` is the full ordered chain.
/// The references chain should be fully assembled via `build_references_chain`
/// before constructing this.
pub(super) struct ThreadingHeaders<'a> {
    pub in_reply_to: &'a str,
    pub references: &'a [String],
}

/// Build the full references chain for threading: existing references + current message ID.
pub(super) fn build_references_chain(original: &OriginalMessage) -> Vec<String> {
    let mut refs = original.references.clone();
    if !original.message_id.is_empty() {
        refs.push(original.message_id.clone());
    }
    refs
}

/// Set threading headers on a `mail_builder::MessageBuilder`.
/// See `ThreadingHeaders` for the bare-ID convention.
pub(super) fn set_threading_headers<'x>(
    mb: mail_builder::MessageBuilder<'x>,
    threading: &ThreadingHeaders<'x>,
) -> mail_builder::MessageBuilder<'x> {
    debug_assert!(
        !threading.in_reply_to.contains('<'),
        "threading IDs must be bare (no angle brackets)"
    );
    debug_assert!(
        threading.references.iter().all(|id| !id.contains('<')),
        "threading IDs must be bare (no angle brackets)"
    );

    use mail_builder::headers::message_id::MessageId;

    let in_reply_to = MessageId::new(threading.in_reply_to);
    let refs = MessageId {
        id: threading
            .references
            .iter()
            .map(|id| id.as_str().into())
            .collect(),
    };

    mb.in_reply_to(in_reply_to).references(refs)
}

/// Apply optional From, CC, and BCC headers to a `MessageBuilder`.
pub(super) fn apply_optional_headers<'x>(
    mut mb: mail_builder::MessageBuilder<'x>,
    from: Option<&'x [Mailbox]>,
    cc: Option<&'x [Mailbox]>,
    bcc: Option<&'x [Mailbox]>,
) -> mail_builder::MessageBuilder<'x> {
    if let Some(from) = from {
        mb = mb.from(to_mb_address_list(from));
    }
    if let Some(cc) = cc {
        mb = mb.cc(to_mb_address_list(cc));
    }
    if let Some(bcc) = bcc {
        mb = mb.bcc(to_mb_address_list(bcc));
    }
    mb
}

/// Set the body (plain or HTML), add any attachments, and write the finished message to a string.
pub(super) fn finalize_message(
    mb: mail_builder::MessageBuilder<'_>,
    body: impl Into<String>,
    html: bool,
    attachments: &[Attachment],
) -> Result<String, GwsError> {
    let mb = if html {
        mb.html_body(body.into())
    } else {
        mb.text_body(body.into())
    };
    let mb = attachments.iter().fold(mb, |mb, att| {
        mb.attachment(&att.content_type, &att.filename, att.data.as_slice())
    });
    mb.write_to_string()
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to serialize email: {e}")))
}

/// Parse an optional clap argument, trimming whitespace and treating
/// empty/whitespace-only values as None.
pub(super) fn parse_optional_trimmed(matches: &ArgMatches, name: &str) -> Option<String> {
    matches
        .get_one::<String>(name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Parse an optional clap argument as a comma-separated mailbox list.
/// Returns `None` when the argument is absent, empty, or yields no valid addresses.
pub(super) fn parse_optional_mailboxes(matches: &ArgMatches, name: &str) -> Option<Vec<Mailbox>> {
    parse_optional_trimmed(matches, name)
        .map(|s| Mailbox::parse_list(&s))
        .filter(|v| !v.is_empty())
}

/// Gmail API upload endpoint limit is 35MB (per discovery document). Messages are
/// sent as multipart/related with the raw RFC 5322 message as the media part, so
/// the limit applies to the entire MIME message including headers, body, and
/// base64-encoded attachments. 25MB raw attachments ≈ 33MB with base64 + overhead.
const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

/// A file attachment read from disk, ready to add to a message.
///
/// `content_type` is inferred from the file extension via `mime_guess2`,
/// falling back to `application/octet-stream` for unknown extensions.
/// `filename` is the basename extracted from the path; mail-builder handles
/// RFC 2231 encoding for non-ASCII filenames in the Content-Disposition header.
#[derive(Debug)]
pub(super) struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// Read and validate attachments from `--attach` arguments.
///
/// Rejects control characters in paths, non-regular files, empty files,
/// and total size exceeding `MAX_TOTAL_ATTACHMENT_BYTES`.
///
/// Absolute and relative paths are both allowed. Unlike `--output-dir` (where
/// write confinement matters), `--attach` only reads files the user's process
/// already has access to. Path traversal restrictions would not prevent data
/// exfiltration — an agent could read any file via other means (e.g., shell
/// commands). The real mitigation for agent misuse is `--dry-run` and human
/// review of the command before execution.
pub(super) fn parse_attachments(matches: &ArgMatches) -> Result<Vec<Attachment>, GwsError> {
    let paths: Vec<&String> = matches
        .get_many::<String>("attach")
        .map(|v| v.collect())
        .unwrap_or_default();

    let mut attachments = Vec::with_capacity(paths.len());
    let mut total_bytes: u64 = 0;

    for path in paths {
        let canonical = crate::validate::validate_safe_file_path(path, "--attach")?;

        let metadata = std::fs::metadata(&canonical)
            .map_err(|e| GwsError::Validation(format!("Cannot read --attach '{path}': {e}")))?;
        if !metadata.is_file() {
            return Err(GwsError::Validation(format!(
                "--attach '{path}' is not a regular file"
            )));
        }

        let data = std::fs::read(&canonical)
            .map_err(|e| GwsError::Validation(format!("Cannot read --attach '{path}': {e}")))?;
        if data.is_empty() {
            return Err(GwsError::Validation(format!(
                "--attach '{path}' is empty (0 bytes)"
            )));
        }
        // Size check uses actual bytes read, not metadata, to avoid TOCTOU race
        total_bytes += data.len() as u64;
        if total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(GwsError::Validation(format!(
                "Total attachment size exceeds {}MB limit",
                MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024)
            )));
        }
        // file_name() is None for paths like "/", "..", or "." — already caught by is_file().
        // to_str() is None only for non-UTF-8 filenames — impossible since path is &String.
        let filename = canonical
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                GwsError::Validation(format!("--attach '{path}': could not extract filename"))
            })?;
        let content_type = mime_guess2::from_path(&canonical)
            .first_or_octet_stream()
            .to_string();

        attachments.push(Attachment {
            filename: filename.to_string(),
            content_type,
            data,
        });
    }

    Ok(attachments)
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

/// Build the JSON metadata for `users.messages.send` via the upload endpoint.
/// Only contains `threadId` when replying/forwarding — the raw RFC 5322 message
/// is sent as the media part, not base64-encoded in a `raw` field.
fn build_send_metadata(thread_id: Option<&str>) -> Option<String> {
    thread_id.map(|id| json!({ "threadId": id }).to_string())
}

pub(super) async fn send_raw_email(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
    raw_message: &str,
    thread_id: Option<&str>,
    existing_token: Option<&str>,
) -> Result<(), GwsError> {
    let metadata = build_send_metadata(thread_id);

    let send_method = resolve_send_method(doc)?;
    let params = json!({ "userId": "me" });
    let params_str = params.to_string();

    let (token, auth_method) = match existing_token {
        Some(t) => (Some(t.to_string()), executor::AuthMethod::OAuth),
        None => {
            let scopes: Vec<&str> = send_method.scopes.iter().map(|s| s.as_str()).collect();
            match auth::get_token(&scopes).await {
                Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                Err(e) if matches.get_flag("dry-run") => {
                    eprintln!("Note: auth skipped for dry-run ({e})");
                    (None, executor::AuthMethod::None)
                }
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
        metadata.as_deref(),
        token.as_deref(),
        auth_method,
        None,
        Some(executor::UploadSource::Bytes {
            data: raw_message.as_bytes(),
            content_type: "message/rfc822",
        }),
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

/// Add --attach, --cc, --bcc, --html, and --dry-run arguments shared by all mail subcommands.
fn common_mail_args(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("attach")
            .short('a')
            .long("attach")
            .help("Attach a file (can be specified multiple times)")
            .action(ArgAction::Append)
            .value_name("PATH"),
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
        Arg::new("html")
            .long("html")
            .help("Treat --body as HTML content (default is plain text)")
            .action(ArgAction::SetTrue),
    )
    .arg(
        Arg::new("dry-run")
            .long("dry-run")
            .help("Show the request that would be sent without executing it")
            .action(ArgAction::SetTrue),
    )
}

/// Add arguments shared by +reply and +reply-all (everything except --remove).
fn common_reply_args(cmd: Command) -> Command {
    common_mail_args(
        cmd.arg(
            Arg::new("message-id")
                .long("message-id")
                .help("Gmail message ID to reply to")
                .required(true)
                .value_name("ID"),
        )
        .arg(
            Arg::new("body")
                .long("body")
                .help("Reply body (plain text, or HTML with --html)")
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
        ),
    )
}

impl Helper for GmailHelper {
    /// Register all Gmail helper subcommands (`+send`, `+reply`, `+reply-all`,
    /// `+forward`, `+triage`, `+watch`) with their arguments and help text.
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            common_mail_args(
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
                            .help("Email body (plain text, or HTML with --html)")
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
                        Arg::new("attachment")
                            .long("attachment")
                            .help("Attach a file (can be repeated for multiple files)")
                            .action(ArgAction::Append)
                            .value_name("PATH"),
                    ),
            )
            .after_help(
                "\
EXAMPLES:
  gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi Alice!'
  gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --cc bob@example.com
  gws gmail +send --to alice@example.com --subject 'Hello' --body '<b>Bold</b> text' --html
  gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --from alias@example.com
  gws gmail +send --to alice@example.com --subject 'Report' --body 'See attached' -a report.pdf
  gws gmail +send --to alice@example.com --subject 'Files' --body 'Two files' -a a.pdf -a b.csv

TIPS:
  Handles RFC 5322 formatting, MIME encoding, and base64 automatically.
  Use --from to send from a configured send-as alias instead of your primary address.
  Use -a/--attach to add file attachments. Can be specified multiple times. Total size limit: 25MB.
  With --html, use fragment tags (<p>, <b>, <a>, <br>, etc.) — no <html>/<body> wrapper needed.",
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
            common_reply_args(
                Command::new("+reply")
                    .about("[Helper] Reply to a message (handles threading automatically)"),
            )
            .after_help(
                "\
EXAMPLES:
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Thanks, got it!'
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Looping in Carol' --cc carol@example.com
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Adding Dave' --to dave@example.com
  gws gmail +reply --message-id 18f1a2b3c4d --body '<b>Bold reply</b>' --html
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Updated version' -a updated.docx

TIPS:
  Automatically sets In-Reply-To, References, and threadId headers.
  Quotes the original message in the reply body.
  --to adds extra recipients to the To field.
  Use -a/--attach to add file attachments. Can be specified multiple times.
  With --html, the quoted block uses Gmail's gmail_quote CSS classes and preserves HTML formatting. \
Use fragment tags (<p>, <b>, <a>, etc.) — no <html>/<body> wrapper needed.
  With --html, inline images in the quoted message (cid: references) will appear broken. \
Externally hosted images are unaffected.
  For reply-all, use +reply-all instead.",
            ),
        );

        cmd = cmd.subcommand(
            common_reply_args(
                Command::new("+reply-all")
                    .about("[Helper] Reply-all to a message (handles threading automatically)"),
            )
            .arg(
                Arg::new("remove")
                    .long("remove")
                    .help("Exclude recipients from the outgoing reply (comma-separated emails)")
                    .value_name("EMAILS"),
            )
            .after_help(
                    "\
EXAMPLES:
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Sounds good to me!'
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Updated' --remove bob@example.com
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Adding Eve' --cc eve@example.com
  gws gmail +reply-all --message-id 18f1a2b3c4d --body '<i>Noted</i>' --html
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Notes attached' -a notes.pdf

TIPS:
  Replies to the sender and all original To/CC recipients.
  Use --to to add extra recipients to the To field.
  Use --cc to add new CC recipients.
  Use --bcc for recipients who should not be visible to others.
  Use --remove to exclude recipients from the outgoing reply, including the sender or Reply-To target.
  The command fails if no To recipient remains after exclusions and --to additions.
  Use -a/--attach to add file attachments. Can be specified multiple times.
  With --html, the quoted block uses Gmail's gmail_quote CSS classes and preserves HTML formatting. \
Use fragment tags (<p>, <b>, <a>, etc.) — no <html>/<body> wrapper needed.
  With --html, inline images in the quoted message (cid: references) will appear broken. \
Externally hosted images are unaffected.",
                ),
        );

        cmd = cmd.subcommand(
            common_mail_args(
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
                        Arg::new("body")
                            .long("body")
                            .help("Optional note to include above the forwarded message (plain text, or HTML with --html)")
                            .value_name("TEXT"),
                    ),
            )
            .after_help(
                    "\
EXAMPLES:
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --body 'FYI see below'
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --cc eve@example.com
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --body '<p>FYI</p>' --html
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com -a notes.pdf

TIPS:
  Includes the original message with sender, date, subject, and recipients.
  Use -a/--attach to add file attachments. Can be specified multiple times.
  With --html, the forwarded block uses Gmail's gmail_quote CSS classes and preserves HTML formatting. \
Use fragment tags (<p>, <b>, <a>, etc.) — no <html>/<body> wrapper needed.
  With --html, inline images in the forwarded message (cid: references) will appear broken. \
Externally hosted images are unaffected.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+read")
                .about("[Helper] Read a message and extract its body or headers")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .alias("message-id")
                        .required(true)
                        .help("The Gmail message ID to read")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("headers")
                        .long("headers")
                        .help("Include headers (From, To, Subject, Date) in the output")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("format")
                        .long("format")
                        .help("Output format (text, json)")
                        .value_parser(["text", "json"])
                        .default_value("text"),
                )
                .arg(
                    Arg::new("html")
                        .long("html")
                        .help("Return HTML body instead of plain text")
                        .action(ArgAction::SetTrue),
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
  gws gmail +read --id 18f1a2b3c4d
  gws gmail +read --id 18f1a2b3c4d --headers
  gws gmail +read --id 18f1a2b3c4d --format json | jq '.body'

TIPS:
  Converts HTML-only messages to plain text automatically.
  Handles multipart/alternative and base64 decoding.",
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

            if let Some(matches) = matches.subcommand_matches("+read") {
                handle_read(doc, matches).await?;
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

    // --- Shared test helpers ---

    /// Extract a header value from raw RFC 5322 output, handling folded lines.
    /// Only searches the header block (before the first blank line).
    pub(super) fn extract_header(raw: &str, name: &str) -> Option<String> {
        let prefix = format!("{}:", name);
        let mut result: Option<String> = None;
        let mut collecting = false;
        for line in raw.lines() {
            // Blank line = end of headers per RFC 5322
            if line.is_empty() || line == "\r" {
                break;
            }
            if line.len() >= prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
                result = Some(line[prefix.len()..].trim().to_string());
                collecting = true;
            } else if collecting && (line.starts_with(' ') || line.starts_with('\t')) {
                if let Some(ref mut r) = result {
                    r.push(' ');
                    r.push_str(line.trim());
                }
            } else {
                collecting = false;
            }
        }
        result
    }

    /// Strip quoted-printable soft line breaks from raw output.
    pub(super) fn strip_qp_soft_breaks(raw: &str) -> String {
        raw.replace("=\r\n", "").replace("=\n", "")
    }

    // --- mail-builder integration tests ---

    #[test]
    fn test_to_mb_address_bare_email() {
        let mailbox = Mailbox::parse("alice@example.com");
        let mut mb = mail_builder::MessageBuilder::new();
        mb = mb
            .to(to_mb_address(&mailbox))
            .subject("test")
            .text_body("body");
        let raw = mb.write_to_string().unwrap();
        let to = extract_header(&raw, "To").unwrap();
        assert!(to.contains("alice@example.com"));
    }

    #[test]
    fn test_to_mb_address_with_display_name() {
        let mailbox = Mailbox::parse("Alice Smith <alice@example.com>");
        let mut mb = mail_builder::MessageBuilder::new();
        mb = mb
            .to(to_mb_address(&mailbox))
            .subject("test")
            .text_body("body");
        let raw = mb.write_to_string().unwrap();
        let to = extract_header(&raw, "To").unwrap();
        assert!(to.contains("alice@example.com"));
        assert!(to.contains("Alice Smith"));
    }

    #[test]
    fn test_to_mb_address_list_multiple() {
        let mailboxes = Mailbox::parse_list("alice@example.com, Bob <bob@example.com>");
        let mut mb = mail_builder::MessageBuilder::new();
        mb = mb
            .to(to_mb_address_list(&mailboxes))
            .subject("test")
            .text_body("body");
        let raw = mb.write_to_string().unwrap();
        let to = extract_header(&raw, "To").unwrap();
        assert!(to.contains("alice@example.com"));
        assert!(to.contains("bob@example.com"));
        assert!(to.contains("Bob"));
    }

    #[test]
    fn test_set_threading_headers_output() {
        let refs = vec![
            "ref-1@example.com".to_string(),
            "ref-2@example.com".to_string(),
        ];
        let threading = ThreadingHeaders {
            in_reply_to: "reply-to@example.com",
            references: &refs,
        };
        let mb = mail_builder::MessageBuilder::new();
        let mb = mb
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test")
            .text_body("body");
        let mb = set_threading_headers(mb, &threading);
        let raw = mb.write_to_string().unwrap();

        let in_reply_to = extract_header(&raw, "In-Reply-To").unwrap();
        assert!(in_reply_to.contains("reply-to@example.com"));

        let references = extract_header(&raw, "References").unwrap();
        assert!(references.contains("ref-1@example.com"));
        assert!(references.contains("ref-2@example.com"));
    }

    // --- OriginalMessage tests ---

    #[test]
    fn test_original_message_default() {
        let d = OriginalMessage::default();
        assert!(d.thread_id.is_none());
        assert!(d.message_id.is_empty());
        assert!(d.references.is_empty());
        assert!(d.from.email.is_empty());
        assert!(d.from.name.is_none());
        assert!(d.reply_to.is_none());
        assert!(d.to.is_empty());
        assert!(d.cc.is_none());
        assert!(d.subject.is_empty());
        assert!(d.date.is_none());
        assert!(d.body_text.is_empty());
        assert!(d.body_html.is_none());
    }

    #[test]
    fn test_parse_original_message_minimal() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "fallback text",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Subject", "value": "Hi" },
                    { "name": "Message-ID", "value": "<min@example.com>" }
                ],
                "body": {
                    "data": URL_SAFE.encode("Hello")
                }
            }
        });
        let original = parse_original_message(&msg).unwrap();
        assert_eq!(original.thread_id.as_deref(), Some("t1"));
        assert_eq!(original.from.email, "alice@example.com");
        assert_eq!(original.subject, "Hi");
        assert_eq!(original.body_text, "Hello");
        assert_eq!(original.message_id, "min@example.com");
        // Missing optional fields default to None/empty
        assert!(original.reply_to.is_none());
        assert!(original.cc.is_none());
        assert!(original.date.is_none());
        assert!(original.references.is_empty());
        assert!(original.body_html.is_none());
    }

    #[test]
    fn test_parse_original_message_bare_message_id() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Subject", "value": "Hi" },
                    { "name": "Message-ID", "value": "bare-id@example.com" }
                ],
                "body": { "data": URL_SAFE.encode("text") }
            }
        });
        let original = parse_original_message(&msg).unwrap();
        // Bare ID (no angle brackets) should be preserved as-is
        assert_eq!(original.message_id, "bare-id@example.com");
    }

    #[test]
    fn test_parse_original_message_missing_payload() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "fallback"
        });
        // Missing payload means no From or Message-ID → error
        let result = parse_original_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_original_message_missing_thread_id() {
        let msg = json!({
            "snippet": "text",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("Hello") }
            }
        });
        let result = parse_original_message(&msg).unwrap();
        assert!(result.thread_id.is_none());
    }

    #[test]
    fn test_parse_original_message_missing_from() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "text",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("Hello") }
            }
        });
        let result = parse_original_message(&msg);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("From"));
    }

    #[test]
    fn test_parse_original_message_missing_message_id() {
        let msg = json!({
            "threadId": "t1",
            "snippet": "text",
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    { "name": "From", "value": "alice@example.com" }
                ],
                "body": { "data": URL_SAFE.encode("Hello") }
            }
        });
        let result = parse_original_message(&msg);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("Message-ID"));
    }

    #[test]
    fn test_parse_original_message_snippet_fallback() {
        // When only text/html is present (no text/plain), body_text falls back to snippet
        let msg = json!({
            "threadId": "t1",
            "snippet": "Snippet fallback text",
            "payload": {
                "mimeType": "text/html",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "body": { "data": URL_SAFE.encode("<p>HTML only</p>") }
            }
        });
        let original = parse_original_message(&msg).unwrap();
        assert_eq!(original.body_text, "Snippet fallback text");
        assert_eq!(original.body_html.unwrap(), "<p>HTML only</p>");
    }

    // --- extract_plain_text_body tests ---

    #[test]
    fn test_extract_plain_text_body_simple() {
        let payload = json!({
            "mimeType": "text/plain",
            "body": {
                "data": URL_SAFE.encode("Hello, world!")
            }
        });
        assert_eq!(extract_plain_text_body(&payload).unwrap(), "Hello, world!");
    }

    #[test]
    fn test_extract_plain_text_body_multipart() {
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {
                    "mimeType": "text/plain",
                    "body": { "data": URL_SAFE.encode("Plain text body") }
                },
                {
                    "mimeType": "text/html",
                    "body": { "data": URL_SAFE.encode("<p>HTML body</p>") }
                }
            ]
        });
        assert_eq!(
            extract_plain_text_body(&payload).unwrap(),
            "Plain text body"
        );
    }

    #[test]
    fn test_extract_plain_text_body_nested_multipart() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                {
                    "mimeType": "multipart/alternative",
                    "parts": [
                        {
                            "mimeType": "text/plain",
                            "body": { "data": URL_SAFE.encode("Nested plain text") }
                        },
                        {
                            "mimeType": "text/html",
                            "body": { "data": URL_SAFE.encode("<p>HTML</p>") }
                        }
                    ]
                },
                {
                    "mimeType": "application/pdf",
                    "body": { "attachmentId": "att123" }
                }
            ]
        });
        assert_eq!(
            extract_plain_text_body(&payload).unwrap(),
            "Nested plain text"
        );
    }

    #[test]
    fn test_extract_plain_text_body_no_text_part() {
        let payload = json!({
            "mimeType": "text/html",
            "body": { "data": URL_SAFE.encode("<p>Only HTML</p>") }
        });
        assert!(extract_plain_text_body(&payload).is_none());
    }

    #[test]
    fn test_inject_commands() {
        let helper = GmailHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();

        let cmd = helper.inject_commands(cmd, &doc);
        let subcommands: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subcommands.contains(&"+watch"));
        assert!(subcommands.contains(&"+send"));
        assert!(subcommands.contains(&"+reply"));
        assert!(subcommands.contains(&"+reply-all"));
        assert!(subcommands.contains(&"+forward"));
        assert!(subcommands.contains(&"+read"));
    }

    #[test]
    fn test_build_send_metadata_with_thread_id() {
        let metadata = build_send_metadata(Some("thread-123")).unwrap();
        let parsed: Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(parsed["threadId"], "thread-123");
    }

    #[test]
    fn test_build_send_metadata_without_thread_id() {
        assert!(build_send_metadata(None).is_none());
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

        let original = parse_original_message(&msg).unwrap();

        assert_eq!(original.thread_id.as_deref(), Some("thread-123"));
        assert_eq!(original.from.email, "alice@example.com");
        let reply_to = original.reply_to.unwrap();
        assert_eq!(reply_to.len(), 2);
        assert_eq!(reply_to[0].email, "team@example.com");
        assert_eq!(reply_to[1].email, "owner@example.com");
        assert_eq!(original.to.len(), 2);
        assert_eq!(original.to[0].email, "bob@example.com");
        assert_eq!(original.to[1].email, "carol@example.com");
        let cc = original.cc.unwrap();
        assert_eq!(cc.len(), 2);
        assert_eq!(cc[0].email, "dave@example.com");
        assert_eq!(cc[1].email, "erin@example.com");
        assert_eq!(original.subject, "Hello");
        assert_eq!(
            original.date.as_deref(),
            Some("Fri, 6 Mar 2026 12:00:00 +0000")
        );
        assert_eq!(original.message_id, "msg@example.com");
        assert_eq!(
            original.references,
            vec!["ref-1@example.com", "ref-2@example.com"]
        );
        assert_eq!(original.body_text, "Snippet fallback");
        assert_eq!(original.body_html.as_deref(), Some("<p>HTML only</p>"));
    }

    #[test]
    fn test_parse_original_message_multipart_alternative() {
        let msg = json!({
            "threadId": "thread-456",
            "snippet": "Snippet ignored when text/plain exists",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    { "name": "From", "value": "alice@example.com" },
                    { "name": "To", "value": "bob@example.com" },
                    { "name": "Subject", "value": "Hello" },
                    { "name": "Date", "value": "Fri, 6 Mar 2026 12:00:00 +0000" },
                    { "name": "Message-ID", "value": "<msg@example.com>" }
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": { "data": URL_SAFE.encode("Plain text body") }
                    },
                    {
                        "mimeType": "text/html",
                        "body": { "data": URL_SAFE.encode("<p>Rich HTML body</p>") }
                    }
                ]
            }
        });

        let original = parse_original_message(&msg).unwrap();

        assert_eq!(original.body_text, "Plain text body");
        assert_eq!(original.body_html.as_deref(), Some("<p>Rich HTML body</p>"));
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

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("Hello World"), "Hello World");
        assert_eq!(
            html_escape("Tom & Jerry <tj@example.com>"),
            "Tom &amp; Jerry &lt;tj@example.com&gt;"
        );
        assert_eq!(
            html_escape("He said \"hello\""),
            "He said &quot;hello&quot;"
        );
        assert_eq!(html_escape("it's"), "it&#39;s");
        assert_eq!(html_escape(""), "");
        assert_eq!(
            html_escape("a & b < c > d \"e\" f'g"),
            "a &amp; b &lt; c &gt; d &quot;e&quot; f&#39;g"
        );
    }

    #[test]
    fn test_extract_html_body_direct() {
        let payload = json!({
            "mimeType": "text/html",
            "body": {
                "data": URL_SAFE.encode("<p>Hello</p>")
            }
        });
        assert_eq!(extract_html_body(&payload).as_deref(), Some("<p>Hello</p>"));
    }

    #[test]
    fn test_extract_html_body_from_multipart() {
        let payload = json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {
                    "mimeType": "text/plain",
                    "body": { "data": URL_SAFE.encode("plain text") }
                },
                {
                    "mimeType": "text/html",
                    "body": { "data": URL_SAFE.encode("<p>rich text</p>") }
                }
            ]
        });
        assert_eq!(
            extract_html_body(&payload).as_deref(),
            Some("<p>rich text</p>")
        );
    }

    #[test]
    fn test_extract_html_body_missing() {
        let payload = json!({
            "mimeType": "text/plain",
            "body": { "data": URL_SAFE.encode("only plain") }
        });
        assert!(extract_html_body(&payload).is_none());
    }

    #[test]
    fn test_extract_html_body_from_nested_multipart() {
        let payload = json!({
            "mimeType": "multipart/mixed",
            "parts": [
                {
                    "mimeType": "multipart/alternative",
                    "parts": [
                        {
                            "mimeType": "text/plain",
                            "body": { "data": URL_SAFE.encode("plain text") }
                        },
                        {
                            "mimeType": "text/html",
                            "body": { "data": URL_SAFE.encode("<p>Nested HTML</p>") }
                        }
                    ]
                },
                {
                    "mimeType": "application/pdf",
                    "body": { "attachmentId": "att123" }
                }
            ]
        });
        assert_eq!(
            extract_html_body(&payload).as_deref(),
            Some("<p>Nested HTML</p>")
        );
    }

    #[test]
    fn test_resolve_html_body_uses_html_when_present() {
        let original = OriginalMessage {
            body_text: "ignored".to_string(),
            body_html: Some("<p>Real HTML</p>".to_string()),
            ..OriginalMessage::dry_run_placeholder("test")
        };
        assert_eq!(resolve_html_body(&original), "<p>Real HTML</p>");
    }

    #[test]
    fn test_resolve_html_body_escapes_plain_text_fallback() {
        let original = OriginalMessage {
            body_text: "Line 1 & <tag>\nLine 2\r\nLine 3".to_string(),
            body_html: None,
            ..OriginalMessage::dry_run_placeholder("test")
        };
        let result = resolve_html_body(&original);
        assert_eq!(
            result,
            "Line 1 &amp; &lt;tag&gt;<br>\r\nLine 2<br>\r\nLine 3"
        );
    }

    // --- Mailbox type tests ---

    #[test]
    fn test_mailbox_parse_bare_email() {
        let m = Mailbox::parse("alice@example.com");
        assert_eq!(m.email, "alice@example.com");
        assert!(m.name.is_none());
    }

    #[test]
    fn test_mailbox_parse_with_display_name() {
        let m = Mailbox::parse("Alice Smith <alice@example.com>");
        assert_eq!(m.email, "alice@example.com");
        assert_eq!(m.name.as_deref(), Some("Alice Smith"));
    }

    #[test]
    fn test_mailbox_parse_quoted_display_name() {
        let m = Mailbox::parse("\"Bob, Jr.\" <bob@example.com>");
        assert_eq!(m.email, "bob@example.com");
        assert_eq!(m.name.as_deref(), Some("Bob, Jr."));
    }

    #[test]
    fn test_mailbox_parse_malformed_no_closing_bracket() {
        let m = Mailbox::parse("Alice <alice@example.com");
        assert_eq!(m.email, "Alice <alice@example.com");
        assert!(m.name.is_none());
    }

    #[test]
    fn test_mailbox_parse_empty() {
        let m = Mailbox::parse("");
        assert_eq!(m.email, "");
        assert!(m.name.is_none());
    }

    #[test]
    fn test_mailbox_parse_empty_angle_brackets() {
        let m = Mailbox::parse("Alice <>");
        // Empty email inside angle brackets
        assert_eq!(m.email, "");
        assert_eq!(m.name.as_deref(), Some("Alice"));
    }

    #[test]
    fn test_mailbox_parse_strips_crlf_injection_in_email() {
        let m = Mailbox::parse("foo@bar.com\r\nBcc: evil@attacker.com");
        assert_eq!(m.email, "foo@bar.comBcc: evil@attacker.com");
        assert!(!m.email.contains('\r'));
        assert!(!m.email.contains('\n'));
    }

    #[test]
    fn test_mailbox_parse_strips_crlf_injection_in_angle_bracket_email() {
        let m = Mailbox::parse("Alice <foo@bar.com\r\nBcc: evil@attacker.com>");
        assert!(!m.email.contains('\r'));
        assert!(!m.email.contains('\n'));
        assert!(m.email.contains("foo@bar.com"));
    }

    #[test]
    fn test_mailbox_parse_strips_control_chars_from_name() {
        let m = Mailbox::parse("Alice\0Bob <alice@example.com>");
        assert_eq!(m.name.as_deref(), Some("AliceBob"));
        assert!(!m.name.unwrap().contains('\0'));
    }

    #[test]
    fn test_mailbox_parse_strips_null_bytes_from_email() {
        let m = Mailbox::parse("alice\0@example.com");
        assert_eq!(m.email, "alice@example.com");
    }

    #[test]
    fn test_mailbox_parse_strips_tab_from_email() {
        let m = Mailbox::parse("alice\t@example.com");
        assert_eq!(m.email, "alice@example.com");
    }

    #[test]
    fn test_mailbox_parse_non_ascii_display_name() {
        let m = Mailbox::parse("田中太郎 <tanaka@example.com>");
        assert_eq!(m.email, "tanaka@example.com");
        assert_eq!(m.name.as_deref(), Some("田中太郎"));

        // Verify non-ASCII name flows through to mail-builder without panic
        // and gets RFC 2047 encoded (replacing hand-rolled encode_address_header from #482)
        let mb = mail_builder::MessageBuilder::new()
            .to(to_mb_address(&m))
            .subject("test")
            .text_body("body");
        let raw = mb.write_to_string().unwrap();
        assert!(raw.contains("tanaka@example.com"));
        assert!(!raw.contains("田中太郎")); // raw CJK should be RFC 2047 encoded
        assert!(raw.contains("=?utf-8?")); // encoded-word present
    }

    #[test]
    fn test_mailbox_parse_list() {
        let list = Mailbox::parse_list("alice@example.com, Bob <bob@example.com>");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].email, "alice@example.com");
        assert_eq!(list[1].email, "bob@example.com");
        assert_eq!(list[1].name.as_deref(), Some("Bob"));
    }

    #[test]
    fn test_mailbox_parse_list_with_quoted_comma() {
        let list = Mailbox::parse_list(r#""Doe, John" <john@example.com>, alice@example.com"#);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].email, "john@example.com");
        assert_eq!(list[0].name.as_deref(), Some("Doe, John"));
        assert_eq!(list[1].email, "alice@example.com");
    }

    #[test]
    fn test_mailbox_parse_list_filters_empty_emails() {
        // Empty string → empty vec
        assert!(Mailbox::parse_list("").is_empty());

        // Whitespace-only commas → empty vec
        assert!(Mailbox::parse_list("  ,  ,  ").is_empty());

        // Trailing comma → no phantom entry
        let list = Mailbox::parse_list("alice@example.com,");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "alice@example.com");

        // Leading comma
        let list = Mailbox::parse_list(",alice@example.com");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "alice@example.com");

        // Empty angle brackets filtered
        let list = Mailbox::parse_list("Alice <>, bob@example.com");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].email, "bob@example.com");
    }

    #[test]
    fn test_mailbox_display() {
        let bare = Mailbox {
            name: None,
            email: "alice@example.com".to_string(),
        };
        assert_eq!(bare.to_string(), "alice@example.com");

        let named = Mailbox {
            name: Some("Alice".to_string()),
            email: "alice@example.com".to_string(),
        };
        assert_eq!(named.to_string(), "Alice <alice@example.com>");
    }

    #[test]
    fn test_strip_angle_brackets() {
        assert_eq!(strip_angle_brackets("<abc@example.com>"), "abc@example.com");
        assert_eq!(strip_angle_brackets("abc@example.com"), "abc@example.com");
        assert_eq!(
            strip_angle_brackets("  <abc@example.com>  "),
            "abc@example.com"
        );
    }

    #[test]
    fn test_build_references_chain() {
        // Empty references + message ID
        let original = OriginalMessage {
            message_id: "msg-1@example.com".to_string(),
            ..Default::default()
        };
        assert_eq!(build_references_chain(&original), vec!["msg-1@example.com"]);

        // Existing references + message ID
        let original = OriginalMessage {
            message_id: "msg-2@example.com".to_string(),
            references: vec![
                "msg-0@example.com".to_string(),
                "msg-1@example.com".to_string(),
            ],
            ..Default::default()
        };
        assert_eq!(
            build_references_chain(&original),
            vec![
                "msg-0@example.com",
                "msg-1@example.com",
                "msg-2@example.com"
            ]
        );

        // Empty message ID doesn't add to chain
        let original = OriginalMessage {
            message_id: String::new(),
            references: vec!["msg-0@example.com".to_string()],
            ..Default::default()
        };
        assert_eq!(build_references_chain(&original), vec!["msg-0@example.com"]);
    }

    // --- HTML fidelity helper tests ---

    #[test]
    fn test_format_sender_for_attribution() {
        // Bare email
        let bare = Mailbox::parse("alice@example.com");
        assert_eq!(
            format_sender_for_attribution(&bare),
            "<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>"
        );
        // Name <email>
        let named = Mailbox::parse("Alice Smith <alice@example.com>");
        assert_eq!(
            format_sender_for_attribution(&named),
            "Alice Smith &lt;<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>&gt;"
        );
        // Special chars in name
        let special = Mailbox::parse("O'Brien & Co <ob@example.com>");
        assert_eq!(
            format_sender_for_attribution(&special),
            "O&#39;Brien &amp; Co &lt;<a href=\"mailto:ob%40example%2Ecom\">ob@example.com</a>&gt;"
        );
    }

    #[test]
    fn test_format_email_link_prevents_mailto_injection() {
        // A crafted email with ?cc= must be percent-encoded in the href so the
        // browser does not interpret it as a mailto parameter.
        let link = format_email_link("user@example.com?cc=evil@attacker.com");
        assert!(link.contains("mailto:"));
        // The href must not contain raw ?cc= (it should be percent-encoded)
        assert!(!link.contains("mailto:user@example.com?cc="));
        assert!(link.contains("%3F")); // ? encoded
        assert!(link.contains("%3D")); // = encoded
    }

    #[test]
    fn test_format_address_list_with_links() {
        let single = vec![Mailbox::parse("alice@example.com")];
        assert_eq!(
            format_address_list_with_links(&single),
            "<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>"
        );
        let multi = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("bob@example.com"),
        ];
        assert_eq!(
            format_address_list_with_links(&multi),
            "<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>, \
             <a href=\"mailto:bob%40example%2Ecom\">bob@example.com</a>"
        );
        let with_name = Mailbox::parse_list(r#""Doe, John" <john@example.com>, alice@example.com"#);
        assert_eq!(
            format_address_list_with_links(&with_name),
            "Doe, John &lt;<a href=\"mailto:john%40example%2Ecom\">john@example.com</a>&gt;, \
             <a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>"
        );
        assert_eq!(format_address_list_with_links(&[]), "");
    }

    #[test]
    fn test_format_date_for_attribution() {
        assert_eq!(
            format_date_for_attribution("Wed, 04 Mar 2026 15:01:00 +0000"),
            "Wed, Mar 4, 2026 at 3:01\u{202f}PM"
        );
        assert_eq!(
            format_date_for_attribution("Jan 1 <2026>"),
            "Jan 1 &lt;2026&gt;"
        );
    }

    #[test]
    fn test_format_forward_from() {
        let named = Mailbox::parse("Alice Smith <alice@example.com>");
        assert_eq!(
            format_forward_from(&named),
            "<strong class=\"gmail_sendername\" dir=\"auto\">Alice Smith</strong> \
             <span dir=\"auto\">&lt;<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>&gt;</span>"
        );
        let bare = Mailbox::parse("alice@example.com");
        assert_eq!(
            format_forward_from(&bare),
            "<strong class=\"gmail_sendername\" dir=\"auto\">alice@example.com</strong> \
             <span dir=\"auto\">&lt;<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a>&gt;</span>"
        );
    }

    #[test]
    fn test_split_raw_mailbox_list() {
        assert_eq!(
            split_raw_mailbox_list("alice@example.com, bob@example.com"),
            vec!["alice@example.com", "bob@example.com"]
        );
        assert_eq!(
            split_raw_mailbox_list("alice@example.com"),
            vec!["alice@example.com"]
        );
        assert!(split_raw_mailbox_list("").is_empty());
        assert_eq!(
            split_raw_mailbox_list(r#""Doe, John" <john@example.com>, alice@example.com"#),
            vec![r#""Doe, John" <john@example.com>"#, "alice@example.com"]
        );
        assert_eq!(
            split_raw_mailbox_list(r#""Doe \"JD, Sr\"" <john@example.com>, alice@example.com"#),
            vec![
                r#""Doe \"JD, Sr\"" <john@example.com>"#,
                "alice@example.com"
            ]
        );
        assert_eq!(
            split_raw_mailbox_list(r#""Trail\\" <t@example.com>, b@example.com"#),
            vec![r#""Trail\\" <t@example.com>"#, "b@example.com"]
        );
    }

    #[test]
    fn test_parse_optional_trimmed() {
        let cmd = Command::new("test")
            .arg(Arg::new("flag").long("flag"))
            .arg(Arg::new("empty").long("empty"))
            .arg(Arg::new("ws").long("ws"));

        // Present, non-empty value
        let matches = cmd
            .clone()
            .try_get_matches_from(["test", "--flag", "value"])
            .unwrap();
        assert_eq!(
            parse_optional_trimmed(&matches, "flag"),
            Some("value".to_string())
        );

        // Absent argument
        let matches = cmd.clone().try_get_matches_from(["test"]).unwrap();
        assert!(parse_optional_trimmed(&matches, "flag").is_none());

        // Whitespace-only becomes None
        let matches = cmd
            .clone()
            .try_get_matches_from(["test", "--ws", "  "])
            .unwrap();
        assert!(parse_optional_trimmed(&matches, "ws").is_none());

        // Empty string becomes None
        let matches = cmd.try_get_matches_from(["test", "--empty", ""]).unwrap();
        assert!(parse_optional_trimmed(&matches, "empty").is_none());
    }

    // --- Attachment tests ---

    fn make_attach_matches(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test").arg(
            Arg::new("attach")
                .short('a')
                .long("attach")
                .action(ArgAction::Append),
        );
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_attachment_single_file() {
        let att = Attachment {
            filename: "report.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            data: b"fake pdf data".to_vec(),
        };
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Body", false, &[att]).unwrap();

        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("report.pdf"));
        assert!(raw.contains("application/pdf"));
        assert!(raw.contains("Body"));
    }

    #[test]
    fn test_attachment_multiple_files() {
        let attachments = vec![
            Attachment {
                filename: "a.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: b"pdf data".to_vec(),
            },
            Attachment {
                filename: "b.csv".to_string(),
                content_type: "text/csv".to_string(),
                data: b"csv data".to_vec(),
            },
        ];
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Body", false, &attachments).unwrap();

        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("a.pdf"));
        assert!(raw.contains("b.csv"));
    }

    #[test]
    fn test_attachment_with_html_body() {
        let att = Attachment {
            filename: "image.png".to_string(),
            content_type: "image/png".to_string(),
            data: vec![0x89, 0x50, 0x4E, 0x47],
        };
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "<p>Hello</p>", true, &[att]).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(raw.contains("multipart/mixed"));
        assert!(decoded.contains("text/html"));
        assert!(decoded.contains("<p>Hello</p>"));
        assert!(raw.contains("image.png"));
    }

    #[test]
    fn test_attachment_empty_produces_no_multipart() {
        let mb = mail_builder::MessageBuilder::new()
            .to(MbAddress::new_address(None::<&str>, "test@example.com"))
            .subject("test");
        let raw = finalize_message(mb, "Body", false, &[]).unwrap();

        assert!(!raw.contains("multipart/mixed"));
        assert!(raw.contains("text/plain"));
    }

    #[test]
    fn test_parse_attachments_rejects_control_chars() {
        let matches = make_attach_matches(&["test", "-a", "file\0name.pdf"]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(err.to_string().contains("control characters"));
    }

    #[test]
    fn test_parse_attachments_rejects_directory() {
        // Use a relative directory that exists in CWD
        let matches = make_attach_matches(&["test", "-a", "src"]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(err.to_string().contains("not a regular file"));
    }

    #[test]
    fn test_parse_attachments_empty_returns_empty_vec() {
        let matches = make_attach_matches(&["test"]);
        let attachments = parse_attachments(&matches).unwrap();
        assert!(attachments.is_empty());
    }

    #[test]
    fn test_parse_attachments_reads_real_file() {
        use std::io::Write;
        let dir = tempfile::tempdir_in(".").unwrap();
        let file_path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);

        let path_str = file_path.to_str().unwrap().to_string();
        let matches = make_attach_matches(&["test", "-a", &path_str]);
        let attachments = parse_attachments(&matches).unwrap();

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "test.txt");
        assert_eq!(attachments[0].content_type, "text/plain");
        assert_eq!(attachments[0].data, b"hello world");
    }

    #[test]
    fn test_parse_attachments_nonexistent_file() {
        let matches = make_attach_matches(&["test", "-a", "nonexistent_file.pdf"]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(
            err.to_string().contains("nonexistent_file.pdf"),
            "error should include the path: {}",
            err
        );
    }

    #[test]
    fn test_parse_attachments_unknown_extension_falls_back_to_octet_stream() {
        use std::io::Write;
        let dir = tempfile::tempdir_in(".").unwrap();
        let file_path = dir.path().join("data.zzqqxx");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"unknown format").unwrap();
        drop(f);

        let path_str = file_path.to_str().unwrap().to_string();
        let matches = make_attach_matches(&["test", "-a", &path_str]);
        let attachments = parse_attachments(&matches).unwrap();

        assert_eq!(attachments[0].content_type, "application/octet-stream");
    }

    #[test]
    fn test_parse_attachments_size_limit_accumulates() {
        use std::io::Write;
        let dir = tempfile::tempdir_in(".").unwrap();

        // Create two files whose combined size exceeds MAX_TOTAL_ATTACHMENT_BYTES
        let file1 = dir.path().join("big1.bin");
        let file2 = dir.path().join("big2.bin");
        // Each file is just over half the limit
        let half_plus_one = (MAX_TOTAL_ATTACHMENT_BYTES / 2 + 1) as usize;
        std::fs::write(&file1, vec![0u8; half_plus_one]).unwrap();
        std::fs::write(&file2, vec![0u8; half_plus_one]).unwrap();

        let path1 = file1.to_str().unwrap().to_string();
        let path2 = file2.to_str().unwrap().to_string();
        let matches = make_attach_matches(&["test", "-a", &path1, "-a", &path2]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(
            err.to_string().contains("exceeds"),
            "error should mention exceeding limit: {}",
            err
        );

        // A single file under the limit should succeed
        let matches = make_attach_matches(&["test", "-a", &path1]);
        assert!(parse_attachments(&matches).is_ok());
    }

    #[test]
    fn test_parse_attachments_rejects_empty_file() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let file_path = dir.path().join("empty.txt");
        std::fs::write(&file_path, b"").unwrap();

        let path_str = file_path.to_str().unwrap().to_string();
        let matches = make_attach_matches(&["test", "-a", &path_str]);
        let err = parse_attachments(&matches).unwrap_err();
        assert!(
            err.to_string().contains("empty (0 bytes)"),
            "error should mention empty file: {}",
            err
        );
    }
}

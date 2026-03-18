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

/// Handle the `+forward` subcommand.
pub(super) async fn handle_forward(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let mut config = parse_forward_args(matches)?;

    let dry_run = matches.get_flag("dry-run");

    let (original, token, client) = if dry_run {
        (
            OriginalMessage::dry_run_placeholder(&config.message_id),
            None,
            None,
        )
    } else {
        let t = auth::get_token(&[GMAIL_SCOPE])
            .await
            .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;
        let c = crate::client::build_client()?;
        let orig = fetch_message_metadata(&c, &t, &config.message_id).await?;
        config.from = resolve_sender(&c, &t, config.from.as_deref()).await?;
        (orig, Some(t), Some(c))
    };

    // Select which original parts to include:
    // - --no-original-attachments: skip regular file attachments, but still
    //   include inline images in HTML mode (they're part of the body, not
    //   "attachments" in the UI sense)
    // - Plain-text mode: drop inline images entirely (matching Gmail web)
    // - HTML mode: include inline images (rendered via cid: in multipart/related)
    let mut all_attachments = config.attachments;
    if let (Some(client), Some(token)) = (&client, &token) {
        let selected: Vec<_> = original
            .parts
            .iter()
            .filter(|p| include_original_part(p, config.html, config.no_original_attachments))
            .cloned()
            .collect();

        fetch_and_merge_original_parts(
            client,
            token,
            &config.message_id,
            &selected,
            &mut all_attachments,
        )
        .await?;
    } else {
        eprintln!("Note: original attachments not included in dry-run preview");
    }

    let subject = build_forward_subject(&original.subject);
    let refs = build_references_chain(&original);
    let envelope = ForwardEnvelope {
        to: &config.to,
        cc: config.cc.as_deref(),
        bcc: config.bcc.as_deref(),
        from: config.from.as_deref(),
        subject: &subject,
        body: config.body.as_deref(),
        html: config.html,
        threading: ThreadingHeaders {
            in_reply_to: &original.message_id,
            references: &refs,
        },
    };

    let raw = create_forward_raw_message(&envelope, &original, &all_attachments)?;

    super::dispatch_raw_email(
        doc,
        matches,
        &raw,
        original.thread_id.as_deref(),
        token.as_deref(),
    )
    .await
}

/// Whether an original MIME part should be included when forwarding.
///
/// - Regular attachments are included unless `--no-original-attachments` is set.
/// - Inline images are included only in HTML mode (matching Gmail web, which
///   strips them from plain-text forwards).
fn include_original_part(part: &OriginalPart, html: bool, no_original_attachments: bool) -> bool {
    if no_original_attachments && !part.is_inline() {
        return false; // skip regular attachments when flag is set
    }
    if !html && part.is_inline() {
        return false; // skip inline images in plain-text mode
    }
    true
}

// --- Data structures ---

pub(super) struct ForwardConfig {
    pub message_id: String,
    pub to: Vec<Mailbox>,
    pub from: Option<Vec<Mailbox>>,
    pub cc: Option<Vec<Mailbox>>,
    pub bcc: Option<Vec<Mailbox>>,
    pub body: Option<String>,
    pub html: bool,
    pub attachments: Vec<Attachment>,
    pub no_original_attachments: bool,
}

struct ForwardEnvelope<'a> {
    to: &'a [Mailbox],
    cc: Option<&'a [Mailbox]>,
    bcc: Option<&'a [Mailbox]>,
    from: Option<&'a [Mailbox]>,
    subject: &'a str,
    body: Option<&'a str>, // Optional user note above forwarded block
    html: bool,            // When true, body and forwarded block are treated as HTML
    threading: ThreadingHeaders<'a>,
}

// --- Message construction ---

fn build_forward_subject(original_subject: &str) -> String {
    if original_subject.to_lowercase().starts_with("fwd:") {
        original_subject.to_string()
    } else {
        format!("Fwd: {}", original_subject)
    }
}

fn create_forward_raw_message(
    envelope: &ForwardEnvelope,
    original: &OriginalMessage,
    attachments: &[Attachment],
) -> Result<String, GwsError> {
    let mb = mail_builder::MessageBuilder::new()
        .to(to_mb_address_list(envelope.to))
        .subject(envelope.subject);

    let mb = apply_optional_headers(mb, envelope.from, envelope.cc, envelope.bcc);
    let mb = set_threading_headers(mb, &envelope.threading);

    let (forwarded_block, separator) = if envelope.html {
        (format_forwarded_message_html(original), "<br>\r\n")
    } else {
        (format_forwarded_message(original), "\r\n\r\n")
    };
    let body = match envelope.body {
        Some(note) => format!("{}{}{}", note, separator, forwarded_block),
        None => forwarded_block,
    };

    finalize_message(mb, body, envelope.html, attachments)
}

/// Join mailboxes into a comma-separated Display string.
fn join_mailboxes(mailboxes: &[Mailbox]) -> String {
    mailboxes
        .iter()
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_forwarded_message(original: &OriginalMessage) -> String {
    let to_str = join_mailboxes(&original.to);
    let date_line = original
        .date
        .as_deref()
        .map(|d| format!("Date: {}\r\n", d))
        .unwrap_or_default();
    let cc_line = original
        .cc
        .as_ref()
        .map(|cc| format!("Cc: {}\r\n", join_mailboxes(cc)))
        .unwrap_or_default();

    format!(
        "---------- Forwarded message ---------\r\n\
         From: {}\r\n\
         {}\
         Subject: {}\r\n\
         To: {}\r\n\
         {}\r\n\
         {}",
        original.from, date_line, original.subject, to_str, cc_line, original.body_text
    )
}

fn format_forwarded_message_html(original: &OriginalMessage) -> String {
    let cc_line = match &original.cc {
        Some(cc) => format!("Cc: {}<br>", format_address_list_with_links(cc)),
        None => String::new(),
    };

    let body = resolve_html_body(original);
    let date_line = match &original.date {
        Some(d) => format!("Date: {}<br>", format_date_for_attribution(d)),
        None => String::new(),
    };
    let from = format_forward_from(&original.from);
    let to = format_address_list_with_links(&original.to);

    format!(
        "<div class=\"gmail_quote gmail_quote_container\">\
           <div dir=\"ltr\" class=\"gmail_attr\">\
             ---------- Forwarded message ---------<br>\
             From: {}<br>\
             {}\
             Subject: {}<br>\
             To: {}<br>\
             {}\
           </div>\
           <br><br>\
           {}\
         </div>",
        from,
        date_line,
        html_escape(&original.subject),
        to,
        cc_line,
        body,
    )
}

// --- Argument parsing ---

fn parse_forward_args(matches: &ArgMatches) -> Result<ForwardConfig, GwsError> {
    let to = Mailbox::parse_list(matches.get_one::<String>("to").unwrap());
    if to.is_empty() {
        return Err(GwsError::Validation(
            "--to must specify at least one recipient".to_string(),
        ));
    }
    Ok(ForwardConfig {
        message_id: matches.get_one::<String>("message-id").unwrap().to_string(),
        to,
        from: parse_optional_mailboxes(matches, "from"),
        cc: parse_optional_mailboxes(matches, "cc"),
        bcc: parse_optional_mailboxes(matches, "bcc"),
        body: parse_optional_trimmed(matches, "body"),
        html: matches.get_flag("html"),
        attachments: parse_attachments(matches)?,
        no_original_attachments: matches.get_flag("no-original-attachments"),
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{extract_header, strip_qp_soft_breaks};
    use super::*;

    // --- format_forwarded_message (plain text) ---

    #[test]
    fn test_format_forwarded_message() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026".to_string()),
            body_text: "Original content".to_string(),
            ..Default::default()
        };
        let msg = format_forwarded_message(&original);
        assert!(msg.contains("---------- Forwarded message ---------"));
        assert!(msg.contains("From: alice@example.com"));
        assert!(msg.contains("Date: Mon, 1 Jan 2026"));
        assert!(msg.contains("Subject: Hello"));
        assert!(msg.contains("To: bob@example.com"));
        assert!(msg.contains("Original content"));
    }

    #[test]
    fn test_format_forwarded_message_missing_date() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            body_text: "Content".to_string(),
            ..Default::default()
        };
        let msg = format_forwarded_message(&original);
        // Date line should be omitted entirely when absent
        assert!(!msg.contains("Date:"));
        // Other lines should still be present
        assert!(msg.contains("From: alice@example.com"));
        assert!(msg.contains("Subject: Hello"));
    }

    #[test]
    fn test_format_forwarded_message_with_cc() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            cc: Some(vec![
                Mailbox::parse("carol@example.com"),
                Mailbox::parse("dave@example.com"),
            ]),
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026".to_string()),
            body_text: "Content".to_string(),
            ..Default::default()
        };
        let msg = format_forwarded_message(&original);
        assert!(msg.contains("Cc: carol@example.com, dave@example.com"));

        // Without CC, no Cc line
        let no_cc = OriginalMessage {
            cc: None,
            ..original
        };
        let msg = format_forwarded_message(&no_cc);
        assert!(!msg.contains("Cc:"));
    }

    // --- forward subject ---

    #[test]
    fn test_build_forward_subject_without_prefix() {
        assert_eq!(build_forward_subject("Hello"), "Fwd: Hello");
    }

    #[test]
    fn test_build_forward_subject_with_prefix() {
        assert_eq!(build_forward_subject("Fwd: Hello"), "Fwd: Hello");
    }

    #[test]
    fn test_build_forward_subject_case_insensitive() {
        assert_eq!(build_forward_subject("FWD: Hello"), "FWD: Hello");
    }

    #[test]
    fn test_create_forward_raw_message_without_body() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original content".to_string(),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: None,
            html: false,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        let raw = create_forward_raw_message(&envelope, &original, &[]).unwrap();

        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("dave@example.com"));
        assert!(extract_header(&raw, "Subject")
            .unwrap()
            .contains("Fwd: Hello"));
        assert!(extract_header(&raw, "In-Reply-To")
            .unwrap()
            .contains("abc@example.com"));
        assert!(raw.contains("---------- Forwarded message ---------"));
        assert!(raw.contains("From: alice@example.com"));
        assert!(raw.contains("Original content"));
    }

    #[test]
    fn test_create_forward_raw_message_with_all_optional_headers() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            cc: Some(vec![Mailbox::parse("carol@example.com")]),
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original content".to_string(),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let cc = Mailbox::parse_list("eve@example.com");
        let bcc = Mailbox::parse_list("secret@example.com");
        let from = Mailbox::parse_list("alias@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: Some(&cc),
            bcc: Some(&bcc),
            from: Some(&from),
            subject: "Fwd: Hello",
            body: Some("FYI see below"),
            html: false,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        let raw = create_forward_raw_message(&envelope, &original, &[]).unwrap();

        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("dave@example.com"));
        assert!(extract_header(&raw, "Cc")
            .unwrap()
            .contains("eve@example.com"));
        assert!(extract_header(&raw, "Bcc")
            .unwrap()
            .contains("secret@example.com"));
        assert!(extract_header(&raw, "From")
            .unwrap()
            .contains("alias@example.com"));
        assert!(raw.contains("FYI see below"));
        assert!(raw.contains("carol@example.com")); // in forwarded block
    }

    #[test]
    fn test_create_forward_raw_message_references_chain() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "msg-2@example.com".to_string(),
            references: vec![
                "msg-0@example.com".to_string(),
                "msg-1@example.com".to_string(),
            ],
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original content".to_string(),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: None,
            html: false,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        let raw = create_forward_raw_message(&envelope, &original, &[]).unwrap();

        // All three message IDs should appear in the References header
        let refs_header = extract_header(&raw, "References").unwrap();
        assert!(refs_header.contains("msg-0@example.com"));
        assert!(refs_header.contains("msg-1@example.com"));
        assert!(refs_header.contains("msg-2@example.com"));
        // In-Reply-To should have only the direct parent
        assert!(extract_header(&raw, "In-Reply-To")
            .unwrap()
            .contains("msg-2@example.com"));
    }

    fn make_forward_matches(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("message-id").long("message-id"))
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("from").long("from"))
            .arg(Arg::new("cc").long("cc"))
            .arg(Arg::new("bcc").long("bcc"))
            .arg(Arg::new("body").long("body"))
            .arg(Arg::new("html").long("html").action(ArgAction::SetTrue))
            .arg(
                Arg::new("attach")
                    .short('a')
                    .long("attach")
                    .action(ArgAction::Append),
            )
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue),
            )
            .arg(
                Arg::new("no-original-attachments")
                    .long("no-original-attachments")
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("draft").long("draft").action(ArgAction::SetTrue));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_forward_args() {
        let matches =
            make_forward_matches(&["test", "--message-id", "abc123", "--to", "dave@example.com"]);
        let config = parse_forward_args(&matches).unwrap();
        assert_eq!(config.message_id, "abc123");
        assert_eq!(config.to[0].email, "dave@example.com");
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
        assert!(config.body.is_none());
        assert!(!config.no_original_attachments);
    }

    #[test]
    fn test_parse_forward_args_no_original_attachments() {
        let matches = make_forward_matches(&[
            "test",
            "--message-id",
            "abc123",
            "--to",
            "dave@example.com",
            "--no-original-attachments",
        ]);
        let config = parse_forward_args(&matches).unwrap();
        assert!(config.no_original_attachments);
    }

    #[test]
    fn test_parse_forward_args_with_all_options() {
        let matches = make_forward_matches(&[
            "test",
            "--message-id",
            "abc123",
            "--to",
            "dave@example.com",
            "--from",
            "alias@example.com",
            "--cc",
            "eve@example.com",
            "--bcc",
            "secret@example.com",
            "--body",
            "FYI",
        ]);
        let config = parse_forward_args(&matches).unwrap();
        assert_eq!(config.from.as_ref().unwrap()[0].email, "alias@example.com");
        assert_eq!(config.cc.as_ref().unwrap()[0].email, "eve@example.com");
        assert_eq!(config.bcc.as_ref().unwrap()[0].email, "secret@example.com");
        assert_eq!(config.body.unwrap(), "FYI");

        // Whitespace-only values become None
        let matches = make_forward_matches(&[
            "test",
            "--message-id",
            "abc123",
            "--to",
            "dave@example.com",
            "--cc",
            "",
            "--bcc",
            "  ",
        ]);
        let config = parse_forward_args(&matches).unwrap();
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
    }

    #[test]
    fn test_parse_forward_args_html_flag() {
        let matches = make_forward_matches(&[
            "test",
            "--message-id",
            "abc123",
            "--to",
            "dave@example.com",
            "--html",
        ]);
        let config = parse_forward_args(&matches).unwrap();
        assert!(config.html);

        // Default is false
        let matches =
            make_forward_matches(&["test", "--message-id", "abc123", "--to", "dave@example.com"]);
        let config = parse_forward_args(&matches).unwrap();
        assert!(!config.html);
    }

    #[test]
    fn test_parse_forward_args_empty_to_returns_error() {
        let matches = make_forward_matches(&["test", "--message-id", "abc123", "--to", ""]);
        let err = parse_forward_args(&matches).err().unwrap();
        assert!(
            err.to_string().contains("--to"),
            "error should mention --to"
        );
    }

    // --- HTML mode tests ---

    #[test]
    fn test_format_forwarded_message_html_with_html_body() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026".to_string()),
            body_text: "plain fallback".to_string(),
            body_html: Some("<p>Rich <b>content</b></p>".to_string()),
            ..Default::default()
        };
        let html = format_forwarded_message_html(&original);
        assert!(html.contains("gmail_quote"));
        assert!(html.contains("Forwarded message"));
        assert!(html.contains("<p>Rich <b>content</b></p>"));
        assert!(!html.contains("plain fallback"));
        // No blockquote in forwards (unlike replies)
        assert!(!html.contains("<blockquote"));
    }

    #[test]
    fn test_format_forwarded_message_html_fallback_plain_text() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026".to_string()),
            body_text: "Line one & <stuff>\nLine two".to_string(),
            ..Default::default()
        };
        let html = format_forwarded_message_html(&original);
        assert!(html.contains("Line one &amp; &lt;stuff&gt;<br>"));
        assert!(html.contains("Line two"));
    }

    #[test]
    fn test_format_forwarded_message_html_escapes_metadata() {
        let original = OriginalMessage {
            from: Mailbox::parse("Tom & Jerry <tj@example.com>"),
            to: vec![Mailbox::parse("<alice@example.com>")],
            subject: "A < B & C".to_string(),
            date: Some("Jan 1 <2026>".to_string()),
            body_text: "text".to_string(),
            ..Default::default()
        };
        let html = format_forwarded_message_html(&original);
        // From line: display name in <strong>, email in mailto link
        assert!(html.contains("Tom &amp; Jerry"));
        assert!(html.contains("<a href=\"mailto:tj%40example%2Ecom\">tj@example.com</a>"));
        // To line: email wrapped in mailto link
        assert!(html.contains("<a href=\"mailto:alice%40example%2Ecom\">"));
        assert!(html.contains("A &lt; B &amp; C"));
        // Non-RFC-2822 date falls back to html-escaped raw string
        assert!(html.contains("Jan 1 &lt;2026&gt;"));
    }

    #[test]
    fn test_format_forwarded_message_html_conditional_cc() {
        let with_cc = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            cc: Some(vec![Mailbox::parse("carol@example.com")]),
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026".to_string()),
            body_text: "text".to_string(),
            ..Default::default()
        };
        let html = format_forwarded_message_html(&with_cc);
        assert!(html.contains("Cc: <a href=\"mailto:carol%40example%2Ecom\">carol@example.com</a>"));

        let without_cc = OriginalMessage {
            cc: None,
            ..with_cc
        };
        let html = format_forwarded_message_html(&without_cc);
        assert!(!html.contains("Cc:"));
    }

    #[test]
    fn test_create_forward_raw_message_html_without_body() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original content".to_string(),
            body_html: Some("<p>Original</p>".to_string()),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: None,
            html: true,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        let raw = create_forward_raw_message(&envelope, &original, &[]).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(decoded.contains("text/html"));
        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("dave@example.com"));
        assert!(decoded.contains("gmail_quote"));
        assert!(decoded.contains("Forwarded message"));
        assert!(decoded.contains("<p>Original</p>"));
    }

    #[test]
    fn test_create_forward_raw_message_html_plain_text_fallback() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Plain & simple".to_string(),
            ..Default::default()
        };
        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: Some("<p>FYI</p>"),
            html: true,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        let raw = create_forward_raw_message(&envelope, &original, &[]).unwrap();

        let decoded = strip_qp_soft_breaks(&raw);
        assert!(decoded.contains("text/html"));
        assert!(decoded.contains("<p>FYI</p>"));
        // Plain text body is HTML-escaped in the fallback
        assert!(decoded.contains("Plain &amp; simple"));
    }

    #[test]
    fn test_create_forward_raw_message_html() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original content".to_string(),
            body_html: Some("<p>Original</p>".to_string()),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: Some("<p>FYI</p>"),
            html: true,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        let raw = create_forward_raw_message(&envelope, &original, &[]).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(decoded.contains("text/html"));
        assert!(decoded.contains("<p>FYI</p>"));
        assert!(decoded.contains("gmail_quote"));
        assert!(decoded.contains("Forwarded message"));
        assert!(decoded.contains("<p>Original</p>"));
    }

    #[test]
    fn test_create_forward_raw_message_with_attachment() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original content".to_string(),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: Some("FYI, see attached"),
            html: false,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        let attachments = vec![Attachment {
            filename: "report.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            data: b"fake pdf".to_vec(),
            content_id: None,
        }];
        let raw = create_forward_raw_message(&envelope, &original, &attachments).unwrap();

        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("report.pdf"));
        assert!(raw.contains("FYI, see attached"));
        assert!(raw.contains("Forwarded message"));
    }

    #[test]
    fn test_create_forward_raw_message_html_with_inline_image() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Photo".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "See photo".to_string(),
            body_html: Some("<p>See <img src=\"cid:baby@example.com\"></p>".to_string()),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Photo",
            body: None,
            html: true,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        // Simulate original inline image + regular attachment
        let attachments = vec![
            Attachment {
                filename: "baby.jpg".to_string(),
                content_type: "image/jpeg".to_string(),
                data: b"fake jpeg".to_vec(),
                content_id: Some("baby@example.com".to_string()),
            },
            Attachment {
                filename: "report.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: b"fake pdf".to_vec(),
                content_id: None,
            },
        ];
        let raw = create_forward_raw_message(&envelope, &original, &attachments).unwrap();

        // Should have multipart/mixed > multipart/related + attachment
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("multipart/related"));
        assert!(raw.contains("Content-ID: <baby@example.com>"));
        assert!(raw.contains("report.pdf"));
    }

    #[test]
    fn test_create_forward_raw_message_plain_text_no_inline_images() {
        // In plain-text mode, inline images are filtered out upstream by the
        // handler (matching Gmail web, which strips them entirely). Only regular
        // attachments reach create_forward_raw_message.
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Photo".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "See photo".to_string(),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = Mailbox::parse_list("dave@example.com");
        let envelope = ForwardEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Photo",
            body: None,
            html: false,
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
        };
        // Only regular attachment — inline images are filtered out by the handler
        let attachments = vec![Attachment {
            filename: "report.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            data: b"fake pdf".to_vec(),
            content_id: None,
        }];
        let raw = create_forward_raw_message(&envelope, &original, &attachments).unwrap();

        assert!(!raw.contains("multipart/related"));
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("report.pdf"));
        // No inline images in plain-text forward
        assert!(!raw.contains("Content-ID"));
    }

    // --- include_original_part filter matrix ---

    fn make_part(inline: bool) -> OriginalPart {
        OriginalPart {
            filename: "test".to_string(),
            content_type: "image/png".to_string(),
            size: 100,
            attachment_id: "ATT1".to_string(),
            content_id: if inline {
                Some("cid@example.com".to_string())
            } else {
                None
            },
        }
    }

    #[test]
    fn test_include_original_part_default_html_includes_all() {
        let regular = make_part(false);
        let inline = make_part(true);
        assert!(include_original_part(&regular, true, false));
        assert!(include_original_part(&inline, true, false));
    }

    #[test]
    fn test_include_original_part_default_plain_drops_inline() {
        let regular = make_part(false);
        let inline = make_part(true);
        assert!(include_original_part(&regular, false, false));
        assert!(!include_original_part(&inline, false, false));
    }

    #[test]
    fn test_include_original_part_no_attachments_html_keeps_inline() {
        let regular = make_part(false);
        let inline = make_part(true);
        // Key behavior: --no-original-attachments skips files but keeps inline images
        assert!(!include_original_part(&regular, true, true));
        assert!(include_original_part(&inline, true, true));
    }

    #[test]
    fn test_include_original_part_no_attachments_plain_drops_everything() {
        let regular = make_part(false);
        let inline = make_part(true);
        assert!(!include_original_part(&regular, false, true));
        assert!(!include_original_part(&inline, false, true));
    }
}

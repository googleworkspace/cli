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
    let config = parse_forward_args(matches)?;

    let dry_run = matches.get_flag("dry-run");

    let (original, token) = if dry_run {
        (
            OriginalMessage::dry_run_placeholder(&config.message_id),
            None,
        )
    } else {
        let t = auth::get_token(&[GMAIL_SCOPE])
            .await
            .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;
        let client = crate::client::build_client()?;
        let orig = fetch_message_metadata(&client, &t, &config.message_id).await?;
        (orig, Some(t))
    };

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

    let raw = create_forward_raw_message(&envelope, &original)?;

    super::send_raw_email(
        doc,
        matches,
        &raw,
        Some(&original.thread_id),
        token.as_deref(),
    )
    .await
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

    finalize_message(mb, body, envelope.html)
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
            thread_id: "t1".to_string(),
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
        let raw = create_forward_raw_message(&envelope, &original).unwrap();

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
            thread_id: "t1".to_string(),
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
        let raw = create_forward_raw_message(&envelope, &original).unwrap();

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
            thread_id: "t1".to_string(),
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
        let raw = create_forward_raw_message(&envelope, &original).unwrap();

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
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue),
            );
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
            thread_id: "t1".to_string(),
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
        let raw = create_forward_raw_message(&envelope, &original).unwrap();
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
            thread_id: "t1".to_string(),
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
        let raw = create_forward_raw_message(&envelope, &original).unwrap();

        let decoded = strip_qp_soft_breaks(&raw);
        assert!(decoded.contains("text/html"));
        assert!(decoded.contains("<p>FYI</p>"));
        // Plain text body is HTML-escaped in the fallback
        assert!(decoded.contains("Plain &amp; simple"));
    }

    #[test]
    fn test_create_forward_raw_message_html() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
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
        let raw = create_forward_raw_message(&envelope, &original).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(decoded.contains("text/html"));
        assert!(decoded.contains("<p>FYI</p>"));
        assert!(decoded.contains("gmail_quote"));
        assert!(decoded.contains("Forwarded message"));
        assert!(decoded.contains("<p>Original</p>"));
    }
}

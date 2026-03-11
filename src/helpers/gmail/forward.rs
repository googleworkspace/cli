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
    let config = parse_forward_args(matches);
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
    let envelope = ForwardEnvelope {
        to: &config.to,
        cc: config.cc.as_deref(),
        bcc: config.bcc.as_deref(),
        from: config.from.as_deref(),
        subject: &subject,
        body: config.body.as_deref(),
        html: config.html,
    };
    let raw = create_forward_raw_message(&envelope, &original);

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
    pub to: String,
    pub from: Option<String>,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub body: Option<String>,
    pub html: bool,
}

struct ForwardEnvelope<'a> {
    to: &'a str,
    cc: Option<&'a str>,
    bcc: Option<&'a str>,
    from: Option<&'a str>,
    subject: &'a str,
    body: Option<&'a str>, // Optional user note above forwarded block
    html: bool,
}

// --- Message construction ---

fn build_forward_subject(original_subject: &str) -> String {
    if original_subject.to_lowercase().starts_with("fwd:") {
        original_subject.to_string()
    } else {
        format!("Fwd: {}", original_subject)
    }
}

fn create_forward_raw_message(envelope: &ForwardEnvelope, original: &OriginalMessage) -> String {
    let references = build_references(&original.references, &original.message_id_header);
    let builder = MessageBuilder {
        to: envelope.to,
        subject: envelope.subject,
        from: envelope.from,
        cc: envelope.cc,
        bcc: envelope.bcc,
        threading: Some(ThreadingHeaders {
            in_reply_to: &original.message_id_header,
            references: &references,
        }),
        html: envelope.html,
    };

    let (forwarded_block, separator) = if envelope.html {
        (format_forwarded_message_html(original), "<br>\r\n")
    } else {
        (format_forwarded_message(original), "\r\n\r\n")
    };
    let body = match envelope.body {
        Some(note) => format!("{}{}{}", note, separator, forwarded_block),
        None => forwarded_block,
    };

    builder.build(&body)
}

fn format_forwarded_message(original: &OriginalMessage) -> String {
    format!(
        "---------- Forwarded message ---------\r\n\
         From: {}\r\n\
         Date: {}\r\n\
         Subject: {}\r\n\
         To: {}\r\n\
         {}\r\n\
         {}",
        original.from,
        original.date,
        original.subject,
        original.to,
        if original.cc.is_empty() {
            String::new()
        } else {
            format!("Cc: {}\r\n", original.cc)
        },
        original.body_text
    )
}

fn format_forwarded_message_html(original: &OriginalMessage) -> String {
    let cc_line = if original.cc.is_empty() {
        String::new()
    } else {
        format!("Cc: {}<br>", format_address_list_with_links(&original.cc))
    };

    let body = resolve_html_body(original);
    let date = format_date_for_attribution(&original.date);
    let from = format_forward_from(&original.from);
    let to = format_address_list_with_links(&original.to);

    format!(
        "<div class=\"gmail_quote gmail_quote_container\">\
           <div dir=\"ltr\" class=\"gmail_attr\">\
             ---------- Forwarded message ---------<br>\
             From: {}<br>\
             Date: {}<br>\
             Subject: {}<br>\
             To: {}<br>\
             {}\
           </div>\
           <br><br>\
           {}\
         </div>",
        from,
        date,
        html_escape(&original.subject),
        to,
        cc_line,
        body,
    )
}

// --- Argument parsing ---

fn parse_forward_args(matches: &ArgMatches) -> ForwardConfig {
    ForwardConfig {
        message_id: matches.get_one::<String>("message-id").unwrap().to_string(),
        to: matches.get_one::<String>("to").unwrap().to_string(),
        from: parse_optional_trimmed(matches, "from"),
        cc: parse_optional_trimmed(matches, "cc"),
        bcc: parse_optional_trimmed(matches, "bcc"),
        body: parse_optional_trimmed(matches, "body"),
        html: matches.get_flag("html"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Original content".to_string(),
            body_html: None,
        };

        let envelope = ForwardEnvelope {
            to: "dave@example.com",
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: None,
            html: false,
        };
        let raw = create_forward_raw_message(&envelope, &original);

        assert!(raw.contains("To: dave@example.com"));
        assert!(raw.contains("Subject: Fwd: Hello"));
        assert!(raw.contains("In-Reply-To: <abc@example.com>"));
        assert!(raw.contains("References: <abc@example.com>"));
        assert!(raw.contains("---------- Forwarded message ---------"));
        assert!(raw.contains("From: alice@example.com"));
        // Blank line separates metadata block from body
        assert!(raw.contains("To: bob@example.com\r\n\r\nOriginal content"));
        // No closing ---------- delimiter
        assert!(!raw.ends_with("----------"));
    }

    #[test]
    fn test_create_forward_raw_message_with_all_optional_headers() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "carol@example.com".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Original content".to_string(),
            body_html: None,
        };

        let envelope = ForwardEnvelope {
            to: "dave@example.com",
            cc: Some("eve@example.com"),
            bcc: Some("secret@example.com"),
            from: Some("alias@example.com"),
            subject: "Fwd: Hello",
            body: Some("FYI see below"),
            html: false,
        };
        let raw = create_forward_raw_message(&envelope, &original);

        assert!(raw.contains("Cc: eve@example.com"));
        assert!(raw.contains("Bcc: secret@example.com"));
        assert!(raw.contains("From: alias@example.com"));
        assert!(raw.contains("FYI see below"));
        assert!(raw.contains("Cc: carol@example.com"));
    }

    #[test]
    fn test_create_forward_raw_message_references_chain() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<msg-2@example.com>".to_string(),
            references: "<msg-0@example.com> <msg-1@example.com>".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Original content".to_string(),
            body_html: None,
        };

        let envelope = ForwardEnvelope {
            to: "dave@example.com",
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: None,
            html: false,
        };
        let raw = create_forward_raw_message(&envelope, &original);

        assert!(raw.contains("In-Reply-To: <msg-2@example.com>"));
        assert!(
            raw.contains("References: <msg-0@example.com> <msg-1@example.com> <msg-2@example.com>")
        );
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
        let config = parse_forward_args(&matches);
        assert_eq!(config.message_id, "abc123");
        assert_eq!(config.to, "dave@example.com");
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
            "--cc",
            "eve@example.com",
            "--bcc",
            "secret@example.com",
            "--body",
            "FYI",
        ]);
        let config = parse_forward_args(&matches);
        assert_eq!(config.cc.unwrap(), "eve@example.com");
        assert_eq!(config.bcc.unwrap(), "secret@example.com");
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
        let config = parse_forward_args(&matches);
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
        let config = parse_forward_args(&matches);
        assert!(config.html);

        // Default is false
        let matches =
            make_forward_matches(&["test", "--message-id", "abc123", "--to", "dave@example.com"]);
        let config = parse_forward_args(&matches);
        assert!(!config.html);
    }

    // --- HTML mode tests ---

    #[test]
    fn test_format_forwarded_message_html_with_html_body() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026".to_string(),
            body_text: "plain fallback".to_string(),
            body_html: Some("<p>Rich <b>content</b></p>".to_string()),
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
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026".to_string(),
            body_text: "Line one & <stuff>\nLine two".to_string(),
            body_html: None,
        };
        let html = format_forwarded_message_html(&original);
        assert!(html.contains("Line one &amp; &lt;stuff&gt;<br>"));
        assert!(html.contains("Line two"));
    }

    #[test]
    fn test_format_forwarded_message_html_escapes_metadata() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "Tom & Jerry <tj@example.com>".to_string(),
            reply_to: "".to_string(),
            to: "<alice@example.com>".to_string(),
            cc: "".to_string(),
            subject: "A < B & C".to_string(),
            date: "Jan 1 <2026>".to_string(),
            body_text: "text".to_string(),
            body_html: None,
        };
        let html = format_forwarded_message_html(&original);
        // From line: display name in <strong>, email in mailto link
        assert!(html.contains("Tom &amp; Jerry"));
        assert!(html.contains("<a href=\"mailto:tj@example.com\">tj@example.com</a>"));
        // To line: email wrapped in mailto link
        assert!(html.contains("<a href=\"mailto:alice@example.com\">"));
        assert!(html.contains("A &lt; B &amp; C"));
        // Non-RFC-2822 date falls back to html-escaped raw string
        assert!(html.contains("Jan 1 &lt;2026&gt;"));
    }

    #[test]
    fn test_format_forwarded_message_html_conditional_cc() {
        let with_cc = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "carol@example.com".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026".to_string(),
            body_text: "text".to_string(),
            body_html: None,
        };
        let html = format_forwarded_message_html(&with_cc);
        assert!(html.contains("Cc: <a href=\"mailto:carol@example.com\">carol@example.com</a>"));

        let without_cc = OriginalMessage {
            cc: "".to_string(),
            ..with_cc
        };
        let html = format_forwarded_message_html(&without_cc);
        assert!(!html.contains("Cc:"));
    }

    #[test]
    fn test_create_forward_raw_message_html_without_body() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Original content".to_string(),
            body_html: Some("<p>Original</p>".to_string()),
        };

        let envelope = ForwardEnvelope {
            to: "dave@example.com",
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: None,
            html: true,
        };
        let raw = create_forward_raw_message(&envelope, &original);

        assert!(raw.contains("Content-Type: text/html; charset=utf-8"));
        assert!(raw.contains("gmail_quote"));
        assert!(raw.contains("Forwarded message"));
        assert!(raw.contains("<p>Original</p>"));
        // No user note — forwarded block is the entire body
        assert!(!raw.contains("<p>FYI</p>"));
    }

    #[test]
    fn test_create_forward_raw_message_html_plain_text_fallback() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Plain & simple".to_string(),
            body_html: None,
        };
        let envelope = ForwardEnvelope {
            to: "dave@example.com",
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: Some("<p>FYI</p>"),
            html: true,
        };
        let raw = create_forward_raw_message(&envelope, &original);

        assert!(raw.contains("Content-Type: text/html; charset=utf-8"));
        assert!(raw.contains("<p>FYI</p><br>\r\n<div class=\"gmail_quote gmail_quote_container\">"));
        // Plain text body is HTML-escaped in the fallback
        assert!(raw.contains("Plain &amp; simple"));
    }

    #[test]
    fn test_create_forward_raw_message_html() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Original content".to_string(),
            body_html: Some("<p>Original</p>".to_string()),
        };

        let envelope = ForwardEnvelope {
            to: "dave@example.com",
            cc: None,
            bcc: None,
            from: None,
            subject: "Fwd: Hello",
            body: Some("<p>FYI</p>"),
            html: true,
        };
        let raw = create_forward_raw_message(&envelope, &original);

        assert!(raw.contains("Content-Type: text/html; charset=utf-8"));
        assert!(raw.contains("<p>FYI</p>"));
        assert!(raw.contains("gmail_quote"));
        assert!(raw.contains("Forwarded message"));
        assert!(raw.contains("<p>Original</p>"));
        // HTML separator: <br> between note and forwarded block (not \r\n\r\n)
        assert!(raw.contains("<p>FYI</p><br>\r\n<div class=\"gmail_quote gmail_quote_container\">"));
    }
}

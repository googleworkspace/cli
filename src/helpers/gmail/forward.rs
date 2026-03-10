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
    let raw = create_forward_raw_message(
        &config.to,
        config.cc.as_deref(),
        config.bcc.as_deref(),
        config.from.as_deref(),
        &subject,
        config.body_text.as_deref(),
        &original,
    );

    super::send_raw_email(
        doc,
        matches,
        &raw,
        Some(&original.thread_id),
        token.as_deref(),
    )
    .await
}

pub(super) struct ForwardConfig {
    pub message_id: String,
    pub to: String,
    pub from: Option<String>,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub body_text: Option<String>,
}

fn build_forward_subject(original_subject: &str) -> String {
    if original_subject.to_lowercase().starts_with("fwd:") {
        original_subject.to_string()
    } else {
        format!("Fwd: {}", original_subject)
    }
}

fn create_forward_raw_message(
    to: &str,
    cc: Option<&str>,
    bcc: Option<&str>,
    from: Option<&str>,
    subject: &str,
    body: Option<&str>,
    original: &OriginalMessage,
) -> String {
    let references = if original.references.is_empty() {
        original.message_id_header.clone()
    } else {
        format!("{} {}", original.references, original.message_id_header)
    };

    let mut headers = format!(
        "To: {}\r\nSubject: {}\r\nIn-Reply-To: {}\r\nReferences: {}\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8",
        to, subject, original.message_id_header, references
    );

    if let Some(from) = from {
        headers.push_str(&format!("\r\nFrom: {}", from));
    }

    if let Some(cc) = cc {
        headers.push_str(&format!("\r\nCc: {}", cc));
    }

    // Gmail API strips the Bcc header from the delivered message.
    if let Some(bcc) = bcc {
        headers.push_str(&format!("\r\nBcc: {}", bcc));
    }

    let forwarded_block = format_forwarded_message(original);

    match body {
        Some(body) => format!("{}\r\n\r\n{}\r\n\r\n{}", headers, body, forwarded_block),
        None => format!("{}\r\n\r\n{}", headers, forwarded_block),
    }
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

fn parse_forward_args(matches: &ArgMatches) -> ForwardConfig {
    ForwardConfig {
        message_id: matches.get_one::<String>("message-id").unwrap().to_string(),
        to: matches.get_one::<String>("to").unwrap().to_string(),
        from: matches.get_one::<String>("from").map(|s| s.to_string()),
        cc: matches
            .get_one::<String>("cc")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        bcc: matches
            .get_one::<String>("bcc")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        body_text: matches.get_one::<String>("body").map(|s| s.to_string()),
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
        let original = super::super::OriginalMessage {
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
        };

        let raw = create_forward_raw_message(
            "dave@example.com",
            None,
            None,
            None,
            "Fwd: Hello",
            None,
            &original,
        );

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
        let original = super::super::OriginalMessage {
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
        };

        let raw = create_forward_raw_message(
            "dave@example.com",
            Some("eve@example.com"),
            Some("secret@example.com"),
            Some("alias@example.com"),
            "Fwd: Hello",
            Some("FYI see below"),
            &original,
        );

        assert!(raw.contains("Cc: eve@example.com"));
        assert!(raw.contains("Bcc: secret@example.com"));
        assert!(raw.contains("From: alias@example.com"));
        assert!(raw.contains("FYI see below"));
        assert!(raw.contains("Cc: carol@example.com"));
    }

    #[test]
    fn test_create_forward_raw_message_references_chain() {
        let original = super::super::OriginalMessage {
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
        };

        let raw = create_forward_raw_message(
            "dave@example.com",
            None,
            None,
            None,
            "Fwd: Hello",
            None,
            &original,
        );

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
        assert!(config.body_text.is_none());
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
        assert_eq!(config.body_text.unwrap(), "FYI");

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

}

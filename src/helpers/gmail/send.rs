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

/// Handle the `+send` subcommand.
pub(super) async fn handle_send(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let mut config = parse_send_args(matches)?;
    let dry_run = matches.get_flag("dry-run");

    let token = if dry_run {
        None
    } else {
        // Use the discovery doc scopes (e.g. gmail.send) rather than hardcoding
        // gmail.modify, so credentials limited to narrower send-only scopes still
        // work. resolve_sender gracefully degrades if the token doesn't cover the
        // sendAs.list endpoint.
        let send_method = super::resolve_send_method(doc)?;
        let scopes: Vec<&str> = send_method.scopes.iter().map(|s| s.as_str()).collect();
        let t = auth::get_token(&scopes)
            .await
            .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;
        let client = crate::client::build_client()?;
        config.from = resolve_sender(&client, &t, config.from.as_deref()).await?;
        Some(t)
    };

    let raw = create_send_raw_message(&config)?;

    super::send_raw_email(doc, matches, &raw, None, token.as_deref()).await
}

pub(super) struct SendConfig {
    pub to: Vec<Mailbox>,
    pub subject: String,
    pub body: String,
    pub from: Option<Vec<Mailbox>>,
    pub cc: Option<Vec<Mailbox>>,
    pub bcc: Option<Vec<Mailbox>>,
    pub html: bool,
    pub attachments: Vec<Attachment>,
}

fn create_send_raw_message(config: &SendConfig) -> Result<String, GwsError> {
    let mb = mail_builder::MessageBuilder::new()
        .to(to_mb_address_list(&config.to))
        .subject(&config.subject);

    let mb = apply_optional_headers(
        mb,
        config.from.as_deref(),
        config.cc.as_deref(),
        config.bcc.as_deref(),
    );

    finalize_message(mb, &config.body, config.html, &config.attachments)
}

fn parse_send_args(matches: &ArgMatches) -> Result<SendConfig, GwsError> {
    let to = Mailbox::parse_list(matches.get_one::<String>("to").unwrap());
    if to.is_empty() {
        return Err(GwsError::Validation(
            "--to must specify at least one recipient".to_string(),
        ));
    }
    Ok(SendConfig {
        to,
        subject: matches.get_one::<String>("subject").unwrap().to_string(),
        body: matches.get_one::<String>("body").unwrap().to_string(),
        from: parse_optional_mailboxes(matches, "from"),
        cc: parse_optional_mailboxes(matches, "cc"),
        bcc: parse_optional_mailboxes(matches, "bcc"),
        html: matches.get_flag("html"),
        attachments: parse_attachments(matches)?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{extract_header, strip_qp_soft_breaks};
    use super::*;

    fn make_matches_send(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("subject").long("subject"))
            .arg(Arg::new("body").long("body"))
            .arg(Arg::new("from").long("from"))
            .arg(Arg::new("cc").long("cc"))
            .arg(Arg::new("bcc").long("bcc"))
            .arg(Arg::new("html").long("html").action(ArgAction::SetTrue))
            .arg(
                Arg::new("attach")
                    .long("attach")
                    .short('a')
                    .action(ArgAction::Append),
            );
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_send_args() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.to.len(), 1);
        assert_eq!(config.to[0].email, "me@example.com");
        assert_eq!(config.subject, "Hi");
        assert_eq!(config.body, "Body");
        assert!(config.from.is_none());
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
    }

    #[test]
    fn test_parse_send_args_with_from() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--from",
            "alias@example.com",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.from.as_ref().unwrap()[0].email, "alias@example.com");

        // Whitespace-only --from becomes None
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--from",
            "  ",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(config.from.is_none());
    }

    #[test]
    fn test_parse_send_args_with_cc_and_bcc() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--cc",
            "carol@example.com",
            "--bcc",
            "secret@example.com",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.cc.as_ref().unwrap()[0].email, "carol@example.com");
        assert_eq!(config.bcc.as_ref().unwrap()[0].email, "secret@example.com");

        // Whitespace-only values become None
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--cc",
            "  ",
            "--bcc",
            "",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
    }

    #[test]
    fn test_parse_send_args_html_flag() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "<b>Bold</b>",
            "--html",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(config.html);

        // Default is false
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Plain",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(!config.html);
    }

    #[test]
    fn test_parse_send_args_empty_to_returns_error() {
        let matches = make_matches_send(&["test", "--to", "", "--subject", "Hi", "--body", "Body"]);
        let err = parse_send_args(&matches).err().unwrap();
        assert!(
            err.to_string().contains("--to"),
            "error should mention --to"
        );
    }

    #[test]
    fn test_send_html_raw_message() {
        let config = SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "HTML test".to_string(),
            body: "<p>Hello <b>world</b></p>".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: true,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(decoded.contains("text/html"));
        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("bob@example.com"));
        assert!(extract_header(&raw, "Subject")
            .unwrap()
            .contains("HTML test"));
        assert!(decoded.contains("<p>Hello <b>world</b></p>"));
        assert!(extract_header(&raw, "Cc").is_none());
    }

    #[test]
    fn test_send_plain_text_raw_message() {
        let config = SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "Hello".to_string(),
            body: "World".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("bob@example.com"));
        assert!(extract_header(&raw, "Subject").unwrap().contains("Hello"));
        assert!(raw.contains("text/plain"));
        assert!(raw.contains("World"));
    }

    #[test]
    fn test_send_with_cc_and_bcc() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: None,
            cc: Some(Mailbox::parse_list("carol@example.com")),
            bcc: Some(Mailbox::parse_list("secret@example.com")),
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("alice@example.com"));
        assert!(extract_header(&raw, "Cc")
            .unwrap()
            .contains("carol@example.com"));
        assert!(extract_header(&raw, "Bcc")
            .unwrap()
            .contains("secret@example.com"));
        // Verify no leakage between headers
        assert!(!extract_header(&raw, "To")
            .unwrap()
            .contains("carol@example.com"));
        assert!(!extract_header(&raw, "To")
            .unwrap()
            .contains("secret@example.com"));
    }

    #[test]
    fn test_send_with_from() {
        let config = SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: Some(Mailbox::parse_list("alias@example.com")),
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(extract_header(&raw, "From")
            .unwrap()
            .contains("alias@example.com"));
        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("bob@example.com"));
    }

    #[test]
    fn test_send_without_from_has_no_from_header() {
        let config = SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(extract_header(&raw, "From").is_none());
    }

    #[test]
    fn test_send_multiple_to_recipients() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com, bob@example.com"),
            subject: "Group".to_string(),
            body: "Hi all".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();
        let to_header = extract_header(&raw, "To").unwrap();
        assert!(to_header.contains("alice@example.com"));
        assert!(to_header.contains("bob@example.com"));
    }

    #[test]
    fn test_send_crlf_injection_in_from_does_not_create_header() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: Some(Mailbox::parse_list(
                "sender@example.com\r\nBcc: evil@attacker.com",
            )),
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        // The CRLF injection should not create a Bcc header
        assert!(
            extract_header(&raw, "Bcc").is_none(),
            "CRLF injection via --from should not create Bcc header"
        );
        // The From header should contain the sanitized email
        assert!(extract_header(&raw, "From")
            .unwrap()
            .contains("sender@example.com"));
    }

    #[test]
    fn test_send_crlf_injection_in_cc_does_not_create_header() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: None,
            cc: Some(Mailbox::parse_list("carol@example.com\r\nX-Injected: yes")),
            bcc: None,
            html: false,
            attachments: vec![],
        };
        let raw = create_send_raw_message(&config).unwrap();

        // CRLF stripped → "X-Injected: yes" is concatenated into the email,
        // not emitted as a separate header line
        assert!(
            extract_header(&raw, "X-Injected").is_none(),
            "CRLF injection via --cc should not create X-Injected header"
        );
        assert!(extract_header(&raw, "Cc")
            .unwrap()
            .contains("carol@example.com"));
    }

    #[test]
    fn test_send_with_attachment_produces_multipart() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Report".to_string(),
            body: "See attached".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![Attachment {
                filename: "report.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: b"fake pdf".to_vec(),
            }],
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("report.pdf"));
        assert!(raw.contains("See attached"));
        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("alice@example.com"));
    }
}

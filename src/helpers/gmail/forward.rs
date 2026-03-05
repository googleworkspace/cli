use super::*;

/// Handle the `+forward` subcommand.
pub(super) async fn handle_forward(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_forward_args(matches);

    let token = auth::get_token(&[GMAIL_SCOPE], None)
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // Fetch original message metadata
    let original =
        super::reply::fetch_message_metadata(&client, &token, &config.message_id).await?;

    let subject = build_forward_subject(&original.subject);
    let raw = create_forward_raw_message(
        &config.to,
        config.cc.as_deref(),
        &subject,
        config.body_text.as_deref(),
        &original,
    );

    super::send_raw_email(doc, matches, &raw, &original.thread_id).await
}

pub struct ForwardConfig {
    pub message_id: String,
    pub to: String,
    pub cc: Option<String>,
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
    subject: &str,
    body: Option<&str>,
    original: &super::reply::OriginalMessage,
) -> String {
    let mut headers = format!("To: {}\r\nSubject: {}", to, subject);

    if let Some(cc) = cc {
        headers.push_str(&format!("\r\nCc: {}", cc));
    }

    let forwarded_block = format_forwarded_message(original);

    match body {
        Some(body) => format!("{}\r\n\r\n{}\r\n\r\n{}", headers, body, forwarded_block),
        None => format!("{}\r\n\r\n{}", headers, forwarded_block),
    }
}

fn format_forwarded_message(original: &super::reply::OriginalMessage) -> String {
    format!(
        "---------- Forwarded message ---------\n\
         From: {}\n\
         Date: {}\n\
         Subject: {}\n\
         To: {}\n\
         {}{}\n\
         ----------",
        original.from,
        original.date,
        original.subject,
        original.to,
        if original.cc.is_empty() {
            String::new()
        } else {
            format!("Cc: {}\n", original.cc)
        },
        original.snippet
    )
}

fn parse_forward_args(matches: &ArgMatches) -> ForwardConfig {
    ForwardConfig {
        message_id: matches.get_one::<String>("message-id").unwrap().to_string(),
        to: matches.get_one::<String>("to").unwrap().to_string(),
        cc: matches.get_one::<String>("cc").map(|s| s.to_string()),
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
        let original = super::super::reply::OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            snippet: "Original content".to_string(),
        };

        let raw =
            create_forward_raw_message("dave@example.com", None, "Fwd: Hello", None, &original);

        assert!(raw.contains("To: dave@example.com"));
        assert!(raw.contains("Subject: Fwd: Hello"));
        assert!(raw.contains("---------- Forwarded message ---------"));
        assert!(raw.contains("From: alice@example.com"));
        assert!(raw.contains("Original content"));
    }

    #[test]
    fn test_create_forward_raw_message_with_body_and_cc() {
        let original = super::super::reply::OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            to: "bob@example.com".to_string(),
            cc: "carol@example.com".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            snippet: "Original content".to_string(),
        };

        let raw = create_forward_raw_message(
            "dave@example.com",
            Some("eve@example.com"),
            "Fwd: Hello",
            Some("FYI see below"),
            &original,
        );

        assert!(raw.contains("Cc: eve@example.com"));
        assert!(raw.contains("FYI see below"));
        assert!(raw.contains("Cc: carol@example.com"));
    }

    fn make_forward_matches(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("message-id").long("message-id"))
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("cc").long("cc"))
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
            "--body",
            "FYI",
        ]);
        let config = parse_forward_args(&matches);
        assert_eq!(config.cc.unwrap(), "eve@example.com");
        assert_eq!(config.body_text.unwrap(), "FYI");
    }
}

use super::*;

/// Handle the `+read` subcommand.
pub(super) async fn handle_read(
    _doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_read_args(matches)?;

    let token = auth::get_token(&[GMAIL_READONLY_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // Reuse the shared fetch helper which includes send_with_retry.
    let parsed = super::fetch_message_metadata(&client, &token, &config.message_id).await?;

    let fmt = matches
        .get_one::<String>("format")
        .map(|s| crate::formatter::OutputFormat::from_str(s))
        .unwrap_or_default();

    let body = if config.html {
        parsed
            .body_html
            .clone()
            .unwrap_or_else(|| parsed.body_text.clone())
    } else {
        parsed.body_text.clone()
    };

    if config.body_only {
        println!("{body}");
    } else {
        let output = json!({
            "id": config.message_id,
            "from": parsed.from,
            "to": parsed.to,
            "cc": parsed.cc,
            "subject": parsed.subject,
            "date": parsed.date,
            "body": body,
        });
        println!("{}", crate::formatter::format_value(&output, &fmt));
    }

    Ok(())
}

#[derive(Debug)]
pub(super) struct ReadConfig {
    pub message_id: String,
    pub html: bool,
    pub body_only: bool,
}

fn parse_read_args(matches: &ArgMatches) -> Result<ReadConfig, GwsError> {
    let message_id = matches.get_one::<String>("message-id").unwrap().to_string();

    if message_id.trim().is_empty() {
        return Err(GwsError::Validation(
            "--message-id must not be empty".to_string(),
        ));
    }

    Ok(ReadConfig {
        message_id,
        html: matches.get_flag("html"),
        body_only: matches.get_flag("body-only"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_matches_read(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("message-id").long("message-id").required(true))
            .arg(Arg::new("html").long("html").action(ArgAction::SetTrue))
            .arg(
                Arg::new("body-only")
                    .long("body-only")
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("format").long("format"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_read_args_basic() {
        let matches = make_matches_read(&["test", "--message-id", "abc123"]);
        let config = parse_read_args(&matches).unwrap();
        assert_eq!(config.message_id, "abc123");
        assert!(!config.html);
        assert!(!config.body_only);
    }

    #[test]
    fn test_parse_read_args_html() {
        let matches = make_matches_read(&["test", "--message-id", "abc123", "--html"]);
        let config = parse_read_args(&matches).unwrap();
        assert!(config.html);
    }

    #[test]
    fn test_parse_read_args_body_only() {
        let matches = make_matches_read(&["test", "--message-id", "abc123", "--body-only"]);
        let config = parse_read_args(&matches).unwrap();
        assert!(config.body_only);
    }

    #[test]
    fn test_parse_read_args_empty_message_id() {
        let matches = make_matches_read(&["test", "--message-id", "  "]);
        let err = parse_read_args(&matches).unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }
}

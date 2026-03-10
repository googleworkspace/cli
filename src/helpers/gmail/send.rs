use super::*;

pub(super) async fn handle_send(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_send_args(matches);

    let raw = MessageBuilder {
        to: &config.to,
        subject: &config.subject,
        from: None,
        cc: config.cc.as_deref(),
        bcc: config.bcc.as_deref(),
        threading: None,
    }
    .build(&config.body_text);

    super::send_raw_email(doc, matches, &raw, None, None).await
}

pub(super) struct SendConfig {
    pub to: String,
    pub subject: String,
    pub body_text: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
}

fn parse_send_args(matches: &ArgMatches) -> SendConfig {
    SendConfig {
        to: matches.get_one::<String>("to").unwrap().to_string(),
        subject: matches.get_one::<String>("subject").unwrap().to_string(),
        body_text: matches.get_one::<String>("body").unwrap().to_string(),
        cc: parse_optional_trimmed(matches, "cc"),
        bcc: parse_optional_trimmed(matches, "bcc"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_matches_send(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("subject").long("subject"))
            .arg(Arg::new("body").long("body"))
            .arg(Arg::new("cc").long("cc"))
            .arg(Arg::new("bcc").long("bcc"));
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
        let config = parse_send_args(&matches);
        assert_eq!(config.to, "me@example.com");
        assert_eq!(config.subject, "Hi");
        assert_eq!(config.body_text, "Body");
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
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
        let config = parse_send_args(&matches);
        assert_eq!(config.cc.unwrap(), "carol@example.com");
        assert_eq!(config.bcc.unwrap(), "secret@example.com");

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
        let config = parse_send_args(&matches);
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
    }
}

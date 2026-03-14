use super::*;

pub(super) async fn handle_send(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_send_args(matches);

    let attachments = match matches.get_many::<String>("attachment") {
        Some(paths) => read_attachments(&paths.cloned().collect::<Vec<_>>())?,
        None => Vec::new(),
    };

    let raw = MessageBuilder {
        to: &config.to,
        subject: &config.subject,
        from: None,
        cc: config.cc.as_deref(),
        bcc: config.bcc.as_deref(),
        threading: None,
        html: config.html,
    }
    .build_with_attachments(&config.body, &attachments);

    super::send_raw_email(doc, matches, &raw, None, None).await
}

pub(super) struct SendConfig {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub html: bool,
}

fn parse_send_args(matches: &ArgMatches) -> SendConfig {
    SendConfig {
        to: matches.get_one::<String>("to").unwrap().to_string(),
        subject: matches.get_one::<String>("subject").unwrap().to_string(),
        body: matches.get_one::<String>("body").unwrap().to_string(),
        cc: parse_optional_trimmed(matches, "cc"),
        bcc: parse_optional_trimmed(matches, "bcc"),
        html: matches.get_flag("html"),
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
            .arg(Arg::new("bcc").long("bcc"))
            .arg(Arg::new("html").long("html").action(ArgAction::SetTrue));
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
        assert_eq!(config.body, "Body");
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
        let config = parse_send_args(&matches);
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
        let config = parse_send_args(&matches);
        assert!(!config.html);
    }

    #[test]
    fn test_send_html_raw_message() {
        let raw = MessageBuilder {
            to: "bob@example.com",
            subject: "HTML test",
            from: None,
            cc: None,
            bcc: None,
            threading: None,
            html: true,
        }
        .build("<p>Hello <b>world</b></p>");

        assert!(raw.contains("Content-Type: text/html; charset=utf-8"));
        assert!(raw.contains("To: bob@example.com"));
        assert!(raw.contains("<p>Hello <b>world</b></p>"));
    }
}

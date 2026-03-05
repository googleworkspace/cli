use super::*;

pub(super) async fn handle_send(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_send_args(matches);

    let message = create_raw_message(&config.to, &config.subject, &config.body_text);
    let body = create_send_body(&message);
    let body_str = body.to_string();

    let users_res = doc
        .resources
        .get("users")
        .ok_or_else(|| GwsError::Discovery("Resource 'users' not found".to_string()))?;
    let messages_res = users_res
        .resources
        .get("messages")
        .ok_or_else(|| GwsError::Discovery("Resource 'users.messages' not found".to_string()))?;
    let send_method = messages_res
        .methods
        .get("send")
        .ok_or_else(|| GwsError::Discovery("Method 'users.messages.send' not found".to_string()))?;

    let pagination = executor::PaginationConfig {
        page_all: false,
        page_limit: 10,
        page_delay_ms: 100,
    };

    let params = json!({ "userId": "me" });
    let params_str = params.to_string();

    let scopes: Vec<&str> = send_method.scopes.iter().map(|s| s.as_str()).collect();
    let (token, auth_method) = match auth::get_token(&scopes, None).await {
        Ok(t) => (Some(t), executor::AuthMethod::OAuth),
        Err(_) => (None, executor::AuthMethod::None),
    };

    executor::execute_method(
        doc,
        send_method,
        Some(&params_str),
        Some(&body_str),
        token.as_deref(),
        auth_method,
        None,
        None,
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

/// Helper to create a raw MIME email string.
fn create_raw_message(to: &str, subject: &str, body: &str) -> String {
    format!("To: {}\r\nSubject: {}\r\n\r\n{}", to, subject, body)
}

/// Creates a JSON body for sending an email.
fn create_send_body(raw_msg: &str) -> serde_json::Value {
    let encoded = URL_SAFE.encode(raw_msg);
    json!({
        "raw": encoded
    })
}

pub struct SendConfig {
    pub to: String,
    pub subject: String,
    pub body_text: String,
}

fn parse_send_args(matches: &ArgMatches) -> SendConfig {
    SendConfig {
        to: matches.get_one::<String>("to").unwrap().to_string(),
        subject: matches.get_one::<String>("subject").unwrap().to_string(),
        body_text: matches.get_one::<String>("body").unwrap().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_raw_message() {
        let msg = create_raw_message("test@example.com", "Hello", "World");
        assert_eq!(msg, "To: test@example.com\r\nSubject: Hello\r\n\r\nWorld");
    }

    #[test]
    fn test_create_send_body() {
        let raw = "To: a@b.com\r\nSubject: hi\r\n\r\nbody";
        let body = create_send_body(raw);
        let encoded = body["raw"].as_str().unwrap();

        let decoded_bytes = URL_SAFE.decode(encoded).unwrap();
        let decoded = String::from_utf8(decoded_bytes).unwrap();

        assert_eq!(decoded, raw);
    }

    fn make_matches_send(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("subject").long("subject"))
            .arg(Arg::new("body").long("body"));
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
    }
}

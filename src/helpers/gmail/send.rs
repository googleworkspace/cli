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
    let (token, auth_method) = match auth::get_token(&scopes).await {
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

/// RFC 2047 encode a header value if it contains non-ASCII characters.
/// Uses standard Base64 (RFC 2045) and folds at 75-char encoded-word limit.
fn encode_header_value(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }

    use base64::engine::general_purpose::STANDARD;

    // RFC 2047 specifies a 75-character limit for encoded-words.
    // Max raw length of 45 bytes -> 60 encoded chars. 60 + len("=?UTF-8?B??=") = 72, < 75.
    const MAX_RAW_LEN: usize = 45;

    // Chunk at character boundaries to avoid splitting multi-byte UTF-8 sequences.
    let mut chunks: Vec<&str> = Vec::new();
    let mut start = 0;
    for (i, ch) in value.char_indices() {
        if i + ch.len_utf8() - start > MAX_RAW_LEN && i > start {
            chunks.push(&value[start..i]);
            start = i;
        }
    }
    if start < value.len() {
        chunks.push(&value[start..]);
    }

    let encoded_words: Vec<String> = chunks
        .iter()
        .map(|chunk| format!("=?UTF-8?B?{}?=", STANDARD.encode(chunk.as_bytes())))
        .collect();

    // Join with CRLF and a space for folding.
    encoded_words.join("\r\n ")
}

/// Helper to create a raw MIME email string.
fn create_raw_message(to: &str, subject: &str, body: &str) -> String {
    format!(
        "MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nTo: {}\r\nSubject: {}\r\n\r\n{}",
        to,
        encode_header_value(subject),
        body
    )
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
    fn test_create_raw_message_ascii() {
        let msg = create_raw_message("test@example.com", "Hello", "World");
        assert_eq!(
            msg,
            "MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nTo: test@example.com\r\nSubject: Hello\r\n\r\nWorld"
        );
    }

    #[test]
    fn test_create_raw_message_non_ascii_subject() {
        let msg = create_raw_message("test@example.com", "Solar — Quote Request", "Body");
        assert!(msg.contains("=?UTF-8?B?"));
        assert!(!msg.contains("Solar — Quote Request"));
    }

    #[test]
    fn test_encode_header_value_ascii() {
        assert_eq!(encode_header_value("Hello World"), "Hello World");
    }

    #[test]
    fn test_encode_header_value_non_ascii_short() {
        let encoded = encode_header_value("Solar — Quote");
        // Single encoded-word, no folding needed
        assert_eq!(encoded, "=?UTF-8?B?U29sYXIg4oCUIFF1b3Rl?=");
    }

    #[test]
    fn test_encode_header_value_non_ascii_long_folds() {
        let long_subject = "This is a very long subject line that contains non-ASCII characters like — and it must be folded to respect the 75-character line limit of RFC 2047.";
        let encoded = encode_header_value(long_subject);

        assert!(encoded.contains("\r\n "), "Encoded string should be folded");
        let parts: Vec<&str> = encoded.split("\r\n ").collect();
        assert!(parts.len() > 1, "Should be multiple parts");
        for part in &parts {
            assert!(part.starts_with("=?UTF-8?B?"));
            assert!(part.ends_with("?="));
            assert!(part.len() <= 75, "Part too long: {} chars", part.len());
        }
    }

    #[test]
    fn test_encode_header_value_multibyte_boundary() {
        // Build a subject where a multi-byte char (€ = 3 bytes) falls near the chunk boundary.
        // Each chunk must decode to valid UTF-8 — no split multi-byte sequences.
        use base64::engine::general_purpose::STANDARD;
        let subject = format!("{}€€€", "A".repeat(43)); // 43 ASCII + 9 bytes of €s = 52 bytes
        let encoded = encode_header_value(&subject);
        for part in encoded.split("\r\n ") {
            let b64 = part.trim_start_matches("=?UTF-8?B?").trim_end_matches("?=");
            let decoded = STANDARD.decode(b64).expect("valid base64");
            String::from_utf8(decoded).expect("each chunk must be valid UTF-8");
        }
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

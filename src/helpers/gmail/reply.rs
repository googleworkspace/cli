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

/// Handle the `+reply` and `+reply-all` subcommands.
pub(super) async fn handle_reply(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
    reply_all: bool,
) -> Result<(), GwsError> {
    let config = parse_reply_args(matches);

    let token = auth::get_token(&[GMAIL_SCOPE], None)
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // Fetch original message metadata
    let original = fetch_message_metadata(&client, &token, &config.message_id).await?;

    // Build reply headers
    let reply_to = if reply_all {
        build_reply_all_recipients(&original, config.cc.as_deref(), config.remove.as_deref())
    } else {
        ReplyRecipients {
            to: extract_reply_to_address(&original),
            cc: config.cc.clone(),
        }
    };

    let subject = build_reply_subject(&original.subject);
    let in_reply_to = original.message_id_header.clone();
    let references = build_references(&original.references, &original.message_id_header);

    let raw = create_reply_raw_message(
        &reply_to.to,
        reply_to.cc.as_deref(),
        &subject,
        &in_reply_to,
        &references,
        &config.body_text,
        &original,
    );

    super::send_raw_email(doc, matches, &raw, &original.thread_id).await
}

// --- Data structures ---

pub(super) struct OriginalMessage {
    pub thread_id: String,
    pub message_id_header: String,
    pub references: String,
    pub from: String,
    pub reply_to: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
}

struct ReplyRecipients {
    to: String,
    cc: Option<String>,
}

pub struct ReplyConfig {
    pub message_id: String,
    pub body_text: String,
    pub cc: Option<String>,
    pub remove: Option<String>,
}

// --- Message fetching ---

pub(super) async fn fetch_message_metadata(
    client: &reqwest::Client,
    token: &str,
    message_id: &str,
) -> Result<OriginalMessage, GwsError> {
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata\
         &metadataHeaders=From&metadataHeaders=To&metadataHeaders=Cc\
         &metadataHeaders=Subject&metadataHeaders=Date\
         &metadataHeaders=Message-ID&metadataHeaders=References\
         &metadataHeaders=Reply-To",
        crate::validate::encode_path_segment(message_id)
    );

    let resp = crate::client::send_with_retry(|| client.get(&url).bearer_auth(token))
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch message: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let err = resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status,
            message: format!("Failed to fetch message {message_id}: {err}"),
            reason: "fetchFailed".to_string(),
            enable_url: None,
        });
    }

    let msg: Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse message: {e}")))?;

    let thread_id = msg
        .get("threadId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let snippet = msg
        .get("snippet")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let headers = msg
        .get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array());

    let mut from = String::new();
    let mut reply_to = String::new();
    let mut to = String::new();
    let mut cc = String::new();
    let mut subject = String::new();
    let mut date = String::new();
    let mut message_id_header = String::new();
    let mut references = String::new();

    if let Some(headers) = headers {
        for h in headers {
            let name = h.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let value = h.get("value").and_then(|v| v.as_str()).unwrap_or("");
            match name {
                "From" => from = value.to_string(),
                "Reply-To" => reply_to = value.to_string(),
                "To" => to = value.to_string(),
                "Cc" => cc = value.to_string(),
                "Subject" => subject = value.to_string(),
                "Date" => date = value.to_string(),
                "Message-ID" | "Message-Id" => message_id_header = value.to_string(),
                "References" => references = value.to_string(),
                _ => {}
            }
        }
    }

    Ok(OriginalMessage {
        thread_id,
        message_id_header,
        references,
        from,
        reply_to,
        to,
        cc,
        subject,
        date,
        snippet,
    })
}

// --- Header construction ---

fn extract_reply_to_address(original: &OriginalMessage) -> String {
    if original.reply_to.is_empty() {
        original.from.clone()
    } else {
        original.reply_to.clone()
    }
}

/// Extract the bare email address from a header value like
/// `"Alice <alice@example.com>"` → `"alice@example.com"` or
/// `"alice@example.com"` → `"alice@example.com"`.
fn extract_email(addr: &str) -> &str {
    if let Some(start) = addr.rfind('<') {
        if let Some(end) = addr[start..].find('>') {
            return &addr[start + 1..start + end];
        }
    }
    addr.trim()
}

fn build_reply_all_recipients(
    original: &OriginalMessage,
    extra_cc: Option<&str>,
    remove: Option<&str>,
) -> ReplyRecipients {
    let to = extract_reply_to_address(original);
    let to_emails: Vec<String> = to
        .split(',')
        .map(|s| extract_email(s.trim()).to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    // Combine original To and Cc for the CC field (excluding the reply-to recipients)
    let mut cc_addrs: Vec<&str> = Vec::new();

    if !original.to.is_empty() {
        for addr in original.to.split(',') {
            let addr = addr.trim();
            if !addr.is_empty() {
                cc_addrs.push(addr);
            }
        }
    }
    if !original.cc.is_empty() {
        for addr in original.cc.split(',') {
            let addr = addr.trim();
            if !addr.is_empty() {
                cc_addrs.push(addr);
            }
        }
    }

    // Add extra CC if provided
    if let Some(extra) = extra_cc {
        for addr in extra.split(',') {
            let addr = addr.trim();
            if !addr.is_empty() {
                cc_addrs.push(addr);
            }
        }
    }

    // Remove addresses if requested (exact email match)
    let remove_set: Vec<String> = remove
        .map(|r| {
            r.split(',')
                .map(|s| extract_email(s).to_lowercase())
                .collect()
        })
        .unwrap_or_default();

    let cc_addrs: Vec<&str> = cc_addrs
        .into_iter()
        .filter(|addr| {
            let email = extract_email(addr).to_lowercase();
            // Filter out the reply-to recipients (already in To) and removed addresses
            !to_emails.iter().any(|t| t == &email)
                && !remove_set.iter().any(|r| r == &email)
        })
        .collect();

    let cc = if cc_addrs.is_empty() {
        None
    } else {
        Some(cc_addrs.join(", "))
    };

    ReplyRecipients { to, cc }
}

fn build_reply_subject(original_subject: &str) -> String {
    if original_subject.to_lowercase().starts_with("re:") {
        original_subject.to_string()
    } else {
        format!("Re: {}", original_subject)
    }
}

fn build_references(original_references: &str, original_message_id: &str) -> String {
    if original_references.is_empty() {
        original_message_id.to_string()
    } else {
        format!("{} {}", original_references, original_message_id)
    }
}

fn create_reply_raw_message(
    to: &str,
    cc: Option<&str>,
    subject: &str,
    in_reply_to: &str,
    references: &str,
    body: &str,
    original: &OriginalMessage,
) -> String {
    let mut headers = format!(
        "To: {}\r\nSubject: {}\r\nIn-Reply-To: {}\r\nReferences: {}",
        to, subject, in_reply_to, references
    );

    if let Some(cc) = cc {
        headers.push_str(&format!("\r\nCc: {}", cc));
    }

    let quoted = format_quoted_original(original);

    format!("{}\r\n\r\n{}\r\n\r\n{}", headers, body, quoted)
}

fn format_quoted_original(original: &OriginalMessage) -> String {
    let quoted_body: String = original
        .snippet
        .lines()
        .map(|line| format!("> {}", line))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "On {}, {} wrote:\n{}",
        original.date, original.from, quoted_body
    )
}

// --- Helpers ---

pub(super) fn resolve_send_method(
    doc: &crate::discovery::RestDescription,
) -> Result<&crate::discovery::RestMethod, GwsError> {
    let users_res = doc
        .resources
        .get("users")
        .ok_or_else(|| GwsError::Discovery("Resource 'users' not found".to_string()))?;
    let messages_res = users_res
        .resources
        .get("messages")
        .ok_or_else(|| GwsError::Discovery("Resource 'users.messages' not found".to_string()))?;
    messages_res
        .methods
        .get("send")
        .ok_or_else(|| GwsError::Discovery("Method 'users.messages.send' not found".to_string()))
}

fn parse_reply_args(matches: &ArgMatches) -> ReplyConfig {
    ReplyConfig {
        message_id: matches.get_one::<String>("message-id").unwrap().to_string(),
        body_text: matches.get_one::<String>("body").unwrap().to_string(),
        cc: matches.get_one::<String>("cc").map(|s| s.to_string()),
        remove: matches.get_one::<String>("remove").map(|s| s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_reply_subject_without_prefix() {
        assert_eq!(build_reply_subject("Hello"), "Re: Hello");
    }

    #[test]
    fn test_build_reply_subject_with_prefix() {
        assert_eq!(build_reply_subject("Re: Hello"), "Re: Hello");
    }

    #[test]
    fn test_build_reply_subject_case_insensitive() {
        assert_eq!(build_reply_subject("RE: Hello"), "RE: Hello");
    }

    #[test]
    fn test_build_references_empty() {
        assert_eq!(
            build_references("", "<msg-1@example.com>"),
            "<msg-1@example.com>"
        );
    }

    #[test]
    fn test_build_references_with_existing() {
        assert_eq!(
            build_references("<msg-0@example.com>", "<msg-1@example.com>"),
            "<msg-0@example.com> <msg-1@example.com>"
        );
    }

    #[test]
    fn test_create_reply_raw_message_basic() {
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
            snippet: "Original body".to_string(),
        };

        let raw = create_reply_raw_message(
            "alice@example.com",
            None,
            "Re: Hello",
            "<abc@example.com>",
            "<abc@example.com>",
            "My reply",
            &original,
        );

        assert!(raw.contains("To: alice@example.com"));
        assert!(raw.contains("Subject: Re: Hello"));
        assert!(raw.contains("In-Reply-To: <abc@example.com>"));
        assert!(raw.contains("References: <abc@example.com>"));
        assert!(raw.contains("My reply"));
        assert!(raw.contains("> Original body"));
    }

    #[test]
    fn test_create_reply_raw_message_with_cc() {
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
            snippet: "Original body".to_string(),
        };

        let raw = create_reply_raw_message(
            "alice@example.com",
            Some("carol@example.com"),
            "Re: Hello",
            "<abc@example.com>",
            "<abc@example.com>",
            "Reply with CC",
            &original,
        );

        assert!(raw.contains("Cc: carol@example.com"));
    }

    #[test]
    fn test_build_reply_all_recipients() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com, carol@example.com".to_string(),
            cc: "dave@example.com".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            snippet: "".to_string(),
        };

        let recipients = build_reply_all_recipients(&original, None, None);
        assert_eq!(recipients.to, "alice@example.com");
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        assert!(cc.contains("carol@example.com"));
        assert!(cc.contains("dave@example.com"));
        // Sender should not be in CC
        assert!(!cc.contains("alice@example.com"));
    }

    #[test]
    fn test_build_reply_all_with_remove() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com, carol@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };

        let recipients = build_reply_all_recipients(&original, None, Some("carol@example.com"));
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        assert!(!cc.contains("carol@example.com"));
    }

    fn make_reply_matches(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("message-id").long("message-id"))
            .arg(Arg::new("body").long("body"))
            .arg(Arg::new("cc").long("cc"))
            .arg(Arg::new("remove").long("remove"))
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue),
            );
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_reply_args() {
        let matches = make_reply_matches(&["test", "--message-id", "abc123", "--body", "My reply"]);
        let config = parse_reply_args(&matches);
        assert_eq!(config.message_id, "abc123");
        assert_eq!(config.body_text, "My reply");
        assert!(config.cc.is_none());
        assert!(config.remove.is_none());
    }

    #[test]
    fn test_parse_reply_args_with_cc_and_remove() {
        let matches = make_reply_matches(&[
            "test",
            "--message-id",
            "abc123",
            "--body",
            "Reply",
            "--cc",
            "extra@example.com",
            "--remove",
            "unwanted@example.com",
        ]);
        let config = parse_reply_args(&matches);
        assert_eq!(config.cc.unwrap(), "extra@example.com");
        assert_eq!(config.remove.unwrap(), "unwanted@example.com");
    }

    #[test]
    fn test_extract_reply_to_address_falls_back_to_from() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "Alice <alice@example.com>".to_string(),
            reply_to: "".to_string(),
            to: "".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        assert_eq!(
            extract_reply_to_address(&original),
            "Alice <alice@example.com>"
        );
    }

    #[test]
    fn test_extract_reply_to_address_prefers_reply_to() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "Alice <alice@example.com>".to_string(),
            reply_to: "list@example.com".to_string(),
            to: "".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        assert_eq!(extract_reply_to_address(&original), "list@example.com");
    }

    #[test]
    fn test_extract_email_bare() {
        assert_eq!(extract_email("alice@example.com"), "alice@example.com");
    }

    #[test]
    fn test_extract_email_with_display_name() {
        assert_eq!(
            extract_email("Alice Smith <alice@example.com>"),
            "alice@example.com"
        );
    }

    #[test]
    fn test_remove_does_not_match_substring() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "sender@example.com".to_string(),
            reply_to: "".to_string(),
            to: "ann@example.com, joann@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        let recipients =
            build_reply_all_recipients(&original, None, Some("ann@example.com"));
        let cc = recipients.cc.unwrap();
        // joann@example.com should remain, ann@example.com should be removed
        assert_eq!(cc, "joann@example.com");
    }

    #[test]
    fn test_reply_all_uses_reply_to_for_to() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "list@example.com".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None);
        assert_eq!(recipients.to, "list@example.com");
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        // list@example.com is in To, should not duplicate in CC
        assert!(!cc.contains("list@example.com"));
    }

    #[test]
    fn test_extract_email_malformed_no_closing_bracket() {
        assert_eq!(extract_email("Alice <alice@example.com"), "Alice <alice@example.com");
    }

    #[test]
    fn test_extract_email_empty_string() {
        assert_eq!(extract_email(""), "");
    }

    #[test]
    fn test_extract_email_whitespace_only() {
        assert_eq!(extract_email("  "), "");
    }

    #[test]
    fn test_sender_with_display_name_excluded_from_cc() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "Alice <alice@example.com>".to_string(),
            reply_to: "".to_string(),
            to: "alice@example.com, bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None);
        assert_eq!(recipients.to, "Alice <alice@example.com>");
        let cc = recipients.cc.unwrap();
        assert_eq!(cc, "bob@example.com");
    }

    #[test]
    fn test_remove_with_display_name_format() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "sender@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com, carol@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        let recipients = build_reply_all_recipients(
            &original,
            None,
            Some("Carol <carol@example.com>"),
        );
        let cc = recipients.cc.unwrap();
        assert_eq!(cc, "bob@example.com");
    }

    #[test]
    fn test_reply_all_with_extra_cc() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        let recipients =
            build_reply_all_recipients(&original, Some("extra@example.com"), None);
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        assert!(cc.contains("extra@example.com"));
    }

    #[test]
    fn test_reply_all_cc_none_when_all_filtered() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "alice@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None);
        assert!(recipients.cc.is_none());
    }

    #[test]
    fn test_case_insensitive_sender_exclusion() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "Alice@Example.COM".to_string(),
            reply_to: "".to_string(),
            to: "alice@example.com, bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None);
        let cc = recipients.cc.unwrap();
        assert_eq!(cc, "bob@example.com");
    }

    #[test]
    fn test_reply_all_multi_address_reply_to_deduplicates_cc() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "list@example.com, owner@example.com".to_string(),
            to: "bob@example.com, list@example.com".to_string(),
            cc: "owner@example.com, dave@example.com".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            snippet: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None);
        // To should be the full Reply-To value
        assert_eq!(recipients.to, "list@example.com, owner@example.com");
        // CC should exclude both Reply-To addresses (already in To)
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        assert!(cc.contains("dave@example.com"));
        assert!(!cc.contains("list@example.com"));
        assert!(!cc.contains("owner@example.com"));
    }
}

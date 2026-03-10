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
    let config = parse_reply_args(matches)?;
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
        let self_email = if reply_all {
            Some(fetch_user_email(&client, &t).await?)
        } else {
            None
        };
        (orig, Some((t, self_email)))
    };

    let self_email = token.as_ref().and_then(|(_, e)| e.as_deref());

    // Determine reply recipients
    let mut reply_to = if reply_all {
        build_reply_all_recipients(
            &original,
            config.cc.as_deref(),
            config.remove.as_deref(),
            self_email,
            config.from.as_deref(),
        )
    } else {
        Ok(ReplyRecipients {
            to: extract_reply_to_address(&original),
            cc: config.cc.clone(),
        })
    }?;

    // Append extra --to recipients
    if let Some(extra_to) = &config.to {
        if reply_to.to.is_empty() {
            reply_to.to = extra_to.clone();
        } else {
            reply_to.to = format!("{}, {}", reply_to.to, extra_to);
        }
    }

    // Dedup across To/CC/BCC (priority: To > CC > BCC)
    let (to, cc, bcc) =
        dedup_recipients(&reply_to.to, reply_to.cc.as_deref(), config.bcc.as_deref());

    if to.is_empty() {
        return Err(GwsError::Validation(
            "No To recipient remains after exclusions and --to additions".to_string(),
        ));
    }

    let subject = build_reply_subject(&original.subject);
    let in_reply_to = original.message_id_header.clone();
    let references = build_references(&original.references, &original.message_id_header);

    let envelope = ReplyEnvelope {
        to: &to,
        cc: cc.as_deref(),
        bcc: bcc.as_deref(),
        from: config.from.as_deref(),
        subject: &subject,
        in_reply_to: &in_reply_to,
        references: &references,
        body: &config.body_text,
    };

    let raw = create_reply_raw_message(&envelope, &original);

    let auth_token = token.as_ref().map(|(t, _)| t.as_str());
    super::send_raw_email(doc, matches, &raw, Some(&original.thread_id), auth_token).await
}

// --- Data structures ---

#[derive(Debug)]
struct ReplyRecipients {
    to: String,
    cc: Option<String>,
}

struct ReplyEnvelope<'a> {
    to: &'a str,
    cc: Option<&'a str>,
    bcc: Option<&'a str>,
    from: Option<&'a str>,
    subject: &'a str,
    in_reply_to: &'a str,
    references: &'a str,
    body: &'a str,
}

pub(super) struct ReplyConfig {
    pub message_id: String,
    pub body_text: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub remove: Option<String>,
}

async fn fetch_user_email(client: &reqwest::Client, token: &str) -> Result<String, GwsError> {
    let resp = crate::client::send_with_retry(|| {
        client
            .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
            .bearer_auth(token)
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch user profile: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let err = resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status,
            message: format!("Failed to fetch user profile: {err}"),
            reason: "profileFetchFailed".to_string(),
            enable_url: None,
        });
    }

    let profile: Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse profile: {e}")))?;

    profile
        .get("emailAddress")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| GwsError::Other(anyhow::anyhow!("Profile missing emailAddress")))
}

// --- Message construction ---

fn extract_reply_to_address(original: &OriginalMessage) -> String {
    if original.reply_to.is_empty() {
        original.from.clone()
    } else {
        original.reply_to.clone()
    }
}

/// Split an RFC 5322 mailbox list on commas, respecting quoted strings.
/// `"Doe, John" <john@example.com>, alice@example.com` →
/// `["\"Doe, John\" <john@example.com>", "alice@example.com"]`
fn split_mailbox_list(header: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    let mut prev_backslash = false;

    for (i, ch) in header.char_indices() {
        match ch {
            '\\' if in_quotes => {
                prev_backslash = !prev_backslash;
                continue;
            }
            '"' if !prev_backslash => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let token = header[start..i].trim();
                if !token.is_empty() {
                    result.push(token);
                }
                start = i + 1;
            }
            _ => {}
        }
        prev_backslash = false;
    }

    let token = header[start..].trim();
    if !token.is_empty() {
        result.push(token);
    }

    result
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
    self_email: Option<&str>,
    from_alias: Option<&str>,
) -> Result<ReplyRecipients, GwsError> {
    let to = extract_reply_to_address(original);
    let excluded = collect_excluded_emails(remove, self_email, from_alias);
    let mut to_emails = std::collections::HashSet::new();
    let to_addrs: Vec<&str> = split_mailbox_list(&to)
        .into_iter()
        .filter(|addr| {
            let email = extract_email(addr).to_lowercase();
            if email.is_empty() || excluded.contains(&email) {
                return false;
            }
            to_emails.insert(email)
        })
        .collect();

    // Combine original To and Cc for the CC field (excluding the reply-to recipients)
    let mut cc_addrs: Vec<&str> = Vec::new();

    if !original.to.is_empty() {
        cc_addrs.extend(split_mailbox_list(&original.to));
    }
    if !original.cc.is_empty() {
        cc_addrs.extend(split_mailbox_list(&original.cc));
    }

    // Add extra CC if provided
    if let Some(extra) = extra_cc {
        cc_addrs.extend(split_mailbox_list(extra));
    }

    // Remove addresses if requested (exact email match)
    let mut seen = std::collections::HashSet::new();
    let cc_addrs: Vec<&str> = cc_addrs
        .into_iter()
        .filter(|addr| {
            let email = extract_email(addr).to_lowercase();
            // Filter out: reply-to recipients, exclusions, and duplicates
            !email.is_empty()
                && !to_emails.contains(&email)
                && !excluded.contains(&email)
                && seen.insert(email)
        })
        .collect();

    let cc = if cc_addrs.is_empty() {
        None
    } else {
        Some(cc_addrs.join(", "))
    };

    Ok(ReplyRecipients {
        to: to_addrs.join(", "),
        cc,
    })
}

/// Deduplicate recipients across To, CC, and BCC fields.
///
/// Priority: To > CC > BCC. If an email appears in multiple fields,
/// it is kept only in the highest-priority field.
fn dedup_recipients(
    to: &str,
    cc: Option<&str>,
    bcc: Option<&str>,
) -> (String, Option<String>, Option<String>) {
    use std::collections::HashSet;

    // Collect To emails into a set
    let mut seen = HashSet::new();
    let to_addrs: Vec<&str> = split_mailbox_list(to)
        .into_iter()
        .filter(|addr| {
            let email = extract_email(addr).to_lowercase();
            !email.is_empty() && seen.insert(email)
        })
        .collect();

    // Filter CC: remove anything already in To
    let cc_addrs: Vec<&str> = cc
        .map(|cc| {
            split_mailbox_list(cc)
                .into_iter()
                .filter(|addr| {
                    let email = extract_email(addr).to_lowercase();
                    !email.is_empty() && seen.insert(email)
                })
                .collect()
        })
        .unwrap_or_default();

    // Filter BCC: remove anything already in To or CC
    let bcc_addrs: Vec<&str> = bcc
        .map(|bcc| {
            split_mailbox_list(bcc)
                .into_iter()
                .filter(|addr| {
                    let email = extract_email(addr).to_lowercase();
                    !email.is_empty() && seen.insert(email)
                })
                .collect()
        })
        .unwrap_or_default();

    let to_out = to_addrs.join(", ");
    let cc_out = if cc_addrs.is_empty() {
        None
    } else {
        Some(cc_addrs.join(", "))
    };
    let bcc_out = if bcc_addrs.is_empty() {
        None
    } else {
        Some(bcc_addrs.join(", "))
    };

    (to_out, cc_out, bcc_out)
}

fn collect_excluded_emails(
    remove: Option<&str>,
    self_email: Option<&str>,
    from_alias: Option<&str>,
) -> std::collections::HashSet<String> {
    let mut excluded = std::collections::HashSet::new();

    if let Some(remove) = remove {
        excluded.extend(
            split_mailbox_list(remove)
                .into_iter()
                .map(extract_email)
                .map(|email| email.to_lowercase())
                .filter(|email| !email.is_empty()),
        );
    }

    if let Some(self_email) = self_email {
        let self_email = extract_email(self_email).to_lowercase();
        if !self_email.is_empty() {
            excluded.insert(self_email);
        }
    }

    if let Some(from_alias) = from_alias {
        let from_alias = extract_email(from_alias).to_lowercase();
        if !from_alias.is_empty() {
            excluded.insert(from_alias);
        }
    }

    excluded
}

fn build_reply_subject(original_subject: &str) -> String {
    if original_subject.to_lowercase().starts_with("re:") {
        original_subject.to_string()
    } else {
        format!("Re: {}", original_subject)
    }
}

fn create_reply_raw_message(envelope: &ReplyEnvelope, original: &OriginalMessage) -> String {
    let builder = MessageBuilder {
        to: envelope.to,
        subject: envelope.subject,
        from: envelope.from,
        cc: envelope.cc,
        bcc: envelope.bcc,
        threading: Some(ThreadingHeaders {
            in_reply_to: envelope.in_reply_to,
            references: envelope.references,
        }),
    };

    let quoted = format_quoted_original(original);
    let body = format!("{}\r\n\r\n{}", envelope.body, quoted);
    builder.build(&body)
}

fn format_quoted_original(original: &OriginalMessage) -> String {
    let quoted_body: String = original
        .body_text
        .lines()
        .map(|line| format!("> {}", line))
        .collect::<Vec<_>>()
        .join("\r\n");

    format!(
        "On {}, {} wrote:\r\n{}",
        original.date, original.from, quoted_body
    )
}

// --- Argument parsing ---

fn parse_reply_args(matches: &ArgMatches) -> Result<ReplyConfig, GwsError> {
    Ok(ReplyConfig {
        message_id: matches.get_one::<String>("message-id").unwrap().to_string(),
        body_text: matches.get_one::<String>("body").unwrap().to_string(),
        from: parse_optional_trimmed(matches, "from"),
        to: parse_optional_trimmed(matches, "to"),
        cc: parse_optional_trimmed(matches, "cc"),
        bcc: parse_optional_trimmed(matches, "bcc"),
        // try_get_one because +reply doesn't define --remove (only +reply-all does).
        // Explicit match distinguishes "arg not defined" from unexpected errors.
        remove: match matches.try_get_one::<String>("remove") {
            Ok(val) => val.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            Err(clap::parser::MatchesError::UnknownArgument { .. }) => None,
            Err(e) => {
                return Err(GwsError::Other(anyhow::anyhow!(
                    "Unexpected error reading --remove argument: {e}"
                )))
            }
        },
    })
}

#[cfg(test)]
mod tests {
    use super::super::extract_plain_text_body;
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
            body_text: "Original body".to_string(),
        };

        let envelope = ReplyEnvelope {
            to: "alice@example.com",
            cc: None,
            bcc: None,
            from: None,
            subject: "Re: Hello",
            in_reply_to: "<abc@example.com>",
            references: "<abc@example.com>",
            body: "My reply",
        };
        let raw = create_reply_raw_message(&envelope, &original);

        assert!(raw.contains("To: alice@example.com"));
        assert!(raw.contains("Subject: Re: Hello"));
        assert!(raw.contains("In-Reply-To: <abc@example.com>"));
        assert!(raw.contains("References: <abc@example.com>"));
        assert!(raw.contains("MIME-Version: 1.0"));
        assert!(raw.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(!raw.contains("From:"));
        assert!(raw.contains("My reply"));
        assert!(raw.contains("> Original body"));
    }

    #[test]
    fn test_create_reply_raw_message_with_all_optional_headers() {
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
            body_text: "Original body".to_string(),
        };

        let envelope = ReplyEnvelope {
            to: "alice@example.com",
            cc: Some("carol@example.com"),
            bcc: Some("secret@example.com"),
            from: Some("alias@example.com"),
            subject: "Re: Hello",
            in_reply_to: "<abc@example.com>",
            references: "<abc@example.com>",
            body: "Reply with all headers",
        };
        let raw = create_reply_raw_message(&envelope, &original);

        assert!(raw.contains("Cc: carol@example.com"));
        assert!(raw.contains("Bcc: secret@example.com"));
        assert!(raw.contains("From: alias@example.com"));
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
            body_text: "".to_string(),
        };

        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
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
            body_text: "".to_string(),
        };

        let recipients =
            build_reply_all_recipients(&original, None, Some("carol@example.com"), None, None)
                .unwrap();
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        assert!(!cc.contains("carol@example.com"));
    }

    #[test]
    fn test_build_reply_all_remove_primary_returns_empty_to() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "".to_string(),
            body_text: "".to_string(),
        };

        let recipients =
            build_reply_all_recipients(&original, None, Some("alice@example.com"), None, None)
                .unwrap();
        assert!(recipients.to.is_empty());
    }

    #[test]
    fn test_reply_all_excludes_from_alias_from_cc() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "sender@example.com".to_string(),
            reply_to: "".to_string(),
            to: "sales@example.com, bob@example.com".to_string(),
            cc: "carol@example.com".to_string(),
            subject: "Hello".to_string(),
            date: "".to_string(),
            body_text: "".to_string(),
        };

        let recipients = build_reply_all_recipients(
            &original,
            None,
            None,
            Some("me@example.com"),
            Some("sales@example.com"),
        )
        .unwrap();
        let cc = recipients.cc.unwrap();

        assert!(!cc.contains("sales@example.com"));
        assert!(cc.contains("bob@example.com"));
        assert!(cc.contains("carol@example.com"));
    }

    #[test]
    fn test_build_reply_all_from_alias_removes_primary_returns_empty_to() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "sales@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "".to_string(),
            body_text: "".to_string(),
        };

        let recipients = build_reply_all_recipients(
            &original,
            None,
            None,
            Some("me@example.com"),
            Some("sales@example.com"),
        )
        .unwrap();
        assert!(recipients.to.is_empty());
    }

    fn make_reply_matches(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("message-id").long("message-id"))
            .arg(Arg::new("body").long("body"))
            .arg(Arg::new("from").long("from"))
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("cc").long("cc"))
            .arg(Arg::new("bcc").long("bcc"))
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
        let config = parse_reply_args(&matches).unwrap();
        assert_eq!(config.message_id, "abc123");
        assert_eq!(config.body_text, "My reply");
        assert!(config.to.is_none());
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
        assert!(config.remove.is_none());
    }

    #[test]
    fn test_parse_reply_args_with_all_options() {
        let matches = make_reply_matches(&[
            "test",
            "--message-id",
            "abc123",
            "--body",
            "Reply",
            "--to",
            "dave@example.com",
            "--cc",
            "extra@example.com",
            "--bcc",
            "secret@example.com",
            "--remove",
            "unwanted@example.com",
        ]);
        let config = parse_reply_args(&matches).unwrap();
        assert_eq!(config.to.unwrap(), "dave@example.com");
        assert_eq!(config.cc.unwrap(), "extra@example.com");
        assert_eq!(config.bcc.unwrap(), "secret@example.com");
        assert_eq!(config.remove.unwrap(), "unwanted@example.com");

        // Whitespace-only values become None
        let matches = make_reply_matches(&[
            "test",
            "--message-id",
            "abc123",
            "--body",
            "Reply",
            "--to",
            "  ",
            "--cc",
            "",
            "--bcc",
            "  ",
        ]);
        let config = parse_reply_args(&matches).unwrap();
        assert!(config.to.is_none());
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
    }

    #[test]
    fn test_parse_reply_args_without_remove_defined() {
        // Simulates +reply which doesn't define --remove (only +reply-all does).
        let cmd = Command::new("test")
            .arg(Arg::new("message-id").long("message-id"))
            .arg(Arg::new("body").long("body"))
            .arg(Arg::new("from").long("from"))
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("cc").long("cc"))
            .arg(Arg::new("bcc").long("bcc"));
        let matches = cmd
            .try_get_matches_from(&["test", "--message-id", "abc", "--body", "hi"])
            .unwrap();
        let config = parse_reply_args(&matches).unwrap();
        assert!(config.remove.is_none());
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
            body_text: "".to_string(),
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
            body_text: "".to_string(),
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
            body_text: "".to_string(),
        };
        let recipients =
            build_reply_all_recipients(&original, None, Some("ann@example.com"), None, None)
                .unwrap();
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
            body_text: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        assert_eq!(recipients.to, "list@example.com");
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        // list@example.com is in To, should not duplicate in CC
        assert!(!cc.contains("list@example.com"));
    }

    #[test]
    fn test_extract_email_malformed_no_closing_bracket() {
        assert_eq!(
            extract_email("Alice <alice@example.com"),
            "Alice <alice@example.com"
        );
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
            body_text: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
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
            body_text: "".to_string(),
        };
        let recipients = build_reply_all_recipients(
            &original,
            None,
            Some("Carol <carol@example.com>"),
            None,
            None,
        )
        .unwrap();
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
            body_text: "".to_string(),
        };
        let recipients =
            build_reply_all_recipients(&original, Some("extra@example.com"), None, None, None)
                .unwrap();
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
            body_text: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
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
            body_text: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
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
            body_text: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        // To should be the full Reply-To value
        assert_eq!(recipients.to, "list@example.com, owner@example.com");
        // CC should exclude both Reply-To addresses (already in To)
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        assert!(cc.contains("dave@example.com"));
        assert!(!cc.contains("list@example.com"));
        assert!(!cc.contains("owner@example.com"));
    }

    #[test]
    fn test_split_mailbox_list_simple() {
        let addrs = split_mailbox_list("alice@example.com, bob@example.com");
        assert_eq!(addrs, vec!["alice@example.com", "bob@example.com"]);
    }

    #[test]
    fn test_split_mailbox_list_quoted_comma() {
        let addrs = split_mailbox_list(r#""Doe, John" <john@example.com>, alice@example.com"#);
        assert_eq!(
            addrs,
            vec![r#""Doe, John" <john@example.com>"#, "alice@example.com"]
        );
    }

    #[test]
    fn test_split_mailbox_list_single() {
        let addrs = split_mailbox_list("alice@example.com");
        assert_eq!(addrs, vec!["alice@example.com"]);
    }

    #[test]
    fn test_split_mailbox_list_empty() {
        let addrs = split_mailbox_list("");
        assert!(addrs.is_empty());
    }

    #[test]
    fn test_split_mailbox_list_escaped_quotes() {
        let addrs = split_mailbox_list(r#""Doe \"JD, Sr\"" <john@example.com>, alice@example.com"#);
        assert_eq!(
            addrs,
            vec![
                r#""Doe \"JD, Sr\"" <john@example.com>"#,
                "alice@example.com"
            ]
        );
    }

    #[test]
    fn test_split_mailbox_list_double_backslash() {
        // \\\\" inside quotes means an escaped backslash followed by a closing quote
        let addrs = split_mailbox_list(r#""Trail\\" <t@example.com>, b@example.com"#);
        assert_eq!(addrs, vec![r#""Trail\\" <t@example.com>"#, "b@example.com"]);
    }

    #[test]
    fn test_reply_all_with_quoted_comma_display_name() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "sender@example.com".to_string(),
            reply_to: "".to_string(),
            to: r#""Doe, John" <john@example.com>, alice@example.com"#.to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            body_text: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        let cc = recipients.cc.unwrap();
        // Both addresses should be preserved intact
        assert!(cc.contains("john@example.com"));
        assert!(cc.contains("alice@example.com"));
    }

    #[test]
    fn test_remove_with_quoted_comma_display_name() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "sender@example.com".to_string(),
            reply_to: "".to_string(),
            to: r#""Doe, John" <john@example.com>, alice@example.com"#.to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            body_text: "".to_string(),
        };
        let recipients =
            build_reply_all_recipients(&original, None, Some("john@example.com"), None, None);
        let cc = recipients.unwrap().cc.unwrap();
        assert!(!cc.contains("john@example.com"));
        assert!(cc.contains("alice@example.com"));
    }

    #[test]
    fn test_reply_all_excludes_self_email() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "me@example.com, bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            body_text: "".to_string(),
        };
        let recipients =
            build_reply_all_recipients(&original, None, None, Some("me@example.com"), None)
                .unwrap();
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        assert!(!cc.contains("me@example.com"));
    }

    #[test]
    fn test_reply_all_excludes_self_case_insensitive() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "Me@Example.COM, bob@example.com".to_string(),
            cc: "".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            body_text: "".to_string(),
        };
        let recipients =
            build_reply_all_recipients(&original, None, None, Some("me@example.com"), None)
                .unwrap();
        let cc = recipients.cc.unwrap();
        assert!(cc.contains("bob@example.com"));
        assert!(!cc.contains("Me@Example.COM"));
    }

    #[test]
    fn test_reply_all_deduplicates_cc() {
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "bob@example.com".to_string(),
            cc: "bob@example.com, carol@example.com".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            body_text: "".to_string(),
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        let cc = recipients.cc.unwrap();
        assert_eq!(cc.matches("bob@example.com").count(), 1);
        assert!(cc.contains("carol@example.com"));
    }

    // --- dedup_recipients tests ---

    #[test]
    fn test_dedup_no_overlap() {
        let (to, cc, bcc) = dedup_recipients(
            "alice@example.com",
            Some("bob@example.com"),
            Some("carol@example.com"),
        );
        assert_eq!(to, "alice@example.com");
        assert_eq!(cc.unwrap(), "bob@example.com");
        assert_eq!(bcc.unwrap(), "carol@example.com");
    }

    #[test]
    fn test_dedup_to_wins_over_cc() {
        let (to, cc, _) = dedup_recipients(
            "alice@example.com",
            Some("alice@example.com, bob@example.com"),
            None,
        );
        assert_eq!(to, "alice@example.com");
        assert_eq!(cc.unwrap(), "bob@example.com");
    }

    #[test]
    fn test_dedup_to_wins_over_bcc() {
        let (to, _, bcc) = dedup_recipients(
            "alice@example.com",
            None,
            Some("alice@example.com, carol@example.com"),
        );
        assert_eq!(to, "alice@example.com");
        assert_eq!(bcc.unwrap(), "carol@example.com");
    }

    #[test]
    fn test_dedup_cc_wins_over_bcc() {
        let (_, cc, bcc) = dedup_recipients(
            "alice@example.com",
            Some("bob@example.com"),
            Some("bob@example.com, carol@example.com"),
        );
        assert_eq!(cc.unwrap(), "bob@example.com");
        assert_eq!(bcc.unwrap(), "carol@example.com");
    }

    #[test]
    fn test_dedup_all_three_overlap() {
        let (to, cc, bcc) = dedup_recipients(
            "alice@example.com",
            Some("alice@example.com, bob@example.com"),
            Some("alice@example.com, bob@example.com, carol@example.com"),
        );
        assert_eq!(to, "alice@example.com");
        assert_eq!(cc.unwrap(), "bob@example.com");
        assert_eq!(bcc.unwrap(), "carol@example.com");
    }

    #[test]
    fn test_dedup_case_insensitive() {
        let (to, cc, _) = dedup_recipients(
            "Alice@Example.COM",
            Some("alice@example.com, bob@example.com"),
            None,
        );
        assert_eq!(to, "Alice@Example.COM");
        assert_eq!(cc.unwrap(), "bob@example.com");
    }

    #[test]
    fn test_dedup_bcc_fully_overlaps_returns_none() {
        let (_, _, bcc) = dedup_recipients(
            "alice@example.com",
            Some("bob@example.com"),
            Some("alice@example.com, bob@example.com"),
        );
        assert!(bcc.is_none());
    }

    #[test]
    fn test_dedup_with_display_names() {
        // Display-name format in To should still dedup against bare email in CC
        let (to, cc, _) = dedup_recipients(
            "Alice <alice@example.com>",
            Some("alice@example.com, bob@example.com"),
            None,
        );
        assert_eq!(to, "Alice <alice@example.com>");
        assert_eq!(cc.unwrap(), "bob@example.com");
    }

    #[test]
    fn test_dedup_intro_pattern() {
        // Intro pattern: remove sender from To, add them to BCC, put CC'd person in To.
        // After build_reply_all_recipients with --remove alice, To is empty, CC has bob.
        // Then --to bob is appended, --bcc alice is set.
        // Dedup should: keep bob in To, remove bob from CC, keep alice in BCC.
        let (to, cc, bcc) = dedup_recipients(
            "bob@example.com",
            Some("bob@example.com"),
            Some("alice@example.com"),
        );
        assert_eq!(to, "bob@example.com");
        assert!(cc.is_none());
        assert_eq!(bcc.unwrap(), "alice@example.com");
    }

    // --- end-to-end --to behavioral tests ---

    #[test]
    fn test_extra_to_appears_in_raw_message() {
        // Simulate +reply with --to dave: reply target is alice, extra To is dave.
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "me@example.com".to_string(),
            cc: "".to_string(),
            subject: "Hello".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Original".to_string(),
        };

        let mut to = extract_reply_to_address(&original);
        let extra_to = "dave@example.com";
        to = format!("{}, {}", to, extra_to);

        let (to, cc, bcc) = dedup_recipients(&to, None, None);

        let envelope = ReplyEnvelope {
            to: &to,
            cc: cc.as_deref(),
            bcc: bcc.as_deref(),
            from: None,
            subject: "Re: Hello",
            in_reply_to: "<abc@example.com>",
            references: "<abc@example.com>",
            body: "Adding Dave",
        };
        let raw = create_reply_raw_message(&envelope, &original);

        assert!(raw.contains("To: alice@example.com, dave@example.com"));
        assert!(!raw.contains("Cc:"));
        assert!(!raw.contains("Bcc:"));
    }

    #[test]
    fn test_intro_pattern_raw_message() {
        // Alice sends to me, CC bob. I reply-all removing alice, adding alice to BCC,
        // and bob to To. Bob should be in To only (deduped from CC), alice in BCC.
        let original = OriginalMessage {
            thread_id: "t1".to_string(),
            message_id_header: "<abc@example.com>".to_string(),
            references: "".to_string(),
            from: "alice@example.com".to_string(),
            reply_to: "".to_string(),
            to: "me@example.com".to_string(),
            cc: "bob@example.com".to_string(),
            subject: "Intro".to_string(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".to_string(),
            body_text: "Meet Bob".to_string(),
        };

        // build_reply_all_recipients with --remove alice, self=me
        let recipients = build_reply_all_recipients(
            &original,
            None,
            Some("alice@example.com"),
            Some("me@example.com"),
            None,
        )
        .unwrap();

        // To is empty (alice removed), CC has bob (me excluded)
        assert!(recipients.to.is_empty());

        // Append --to bob
        let to = "bob@example.com".to_string();

        // Dedup with --bcc alice
        let (to, cc, bcc) =
            dedup_recipients(&to, recipients.cc.as_deref(), Some("alice@example.com"));

        let envelope = ReplyEnvelope {
            to: &to,
            cc: cc.as_deref(),
            bcc: bcc.as_deref(),
            from: None,
            subject: "Re: Intro",
            in_reply_to: "<abc@example.com>",
            references: "<abc@example.com>",
            body: "Hi Bob, nice to meet you!",
        };
        let raw = create_reply_raw_message(&envelope, &original);

        assert!(raw.contains("To: bob@example.com"));
        assert!(!raw.contains("Cc:"));
        assert!(raw.contains("Bcc: alice@example.com"));
        assert!(raw.contains("Hi Bob, nice to meet you!"));
    }

    #[test]
    fn test_extract_plain_text_body_simple() {
        let payload = serde_json::json!({
            "mimeType": "text/plain",
            "body": {
                "data": URL_SAFE.encode("Hello, world!")
            }
        });
        assert_eq!(extract_plain_text_body(&payload).unwrap(), "Hello, world!");
    }

    #[test]
    fn test_extract_plain_text_body_multipart() {
        let payload = serde_json::json!({
            "mimeType": "multipart/alternative",
            "parts": [
                {
                    "mimeType": "text/plain",
                    "body": {
                        "data": URL_SAFE.encode("Plain text body")
                    }
                },
                {
                    "mimeType": "text/html",
                    "body": {
                        "data": URL_SAFE.encode("<p>HTML body</p>")
                    }
                }
            ]
        });
        assert_eq!(
            extract_plain_text_body(&payload).unwrap(),
            "Plain text body"
        );
    }

    #[test]
    fn test_extract_plain_text_body_nested_multipart() {
        let payload = serde_json::json!({
            "mimeType": "multipart/mixed",
            "parts": [
                {
                    "mimeType": "multipart/alternative",
                    "parts": [
                        {
                            "mimeType": "text/plain",
                            "body": {
                                "data": URL_SAFE.encode("Nested plain text")
                            }
                        },
                        {
                            "mimeType": "text/html",
                            "body": {
                                "data": URL_SAFE.encode("<p>HTML</p>")
                            }
                        }
                    ]
                },
                {
                    "mimeType": "application/pdf",
                    "body": { "attachmentId": "att123" }
                }
            ]
        });
        assert_eq!(
            extract_plain_text_body(&payload).unwrap(),
            "Nested plain text"
        );
    }

    #[test]
    fn test_extract_plain_text_body_no_text_part() {
        let payload = serde_json::json!({
            "mimeType": "text/html",
            "body": {
                "data": URL_SAFE.encode("<p>Only HTML</p>")
            }
        });
        assert!(extract_plain_text_body(&payload).is_none());
    }
}

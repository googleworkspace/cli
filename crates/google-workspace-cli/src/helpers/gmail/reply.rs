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
    let mut config = parse_reply_args(matches)?;
    let dry_run = matches.get_flag("dry-run");

    let (original, token, self_email, client) = if dry_run {
        (
            OriginalMessage::dry_run_placeholder(&config.message_id),
            None,
            None,
            None,
        )
    } else {
        let t = auth::get_token(&[GMAIL_SCOPE])
            .await
            .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;
        let c = crate::client::build_client()?;
        let orig = fetch_message_metadata(&c, &t, &config.message_id).await?;
        config.from = resolve_sender(&c, &t, config.from.as_deref()).await?;
        // For reply-all, always fetch the primary email for self-dedup and
        // self-reply detection. The resolved sender may be an alias that differs from the primary
        // address — both must be excluded from recipients. from_alias_email
        // (extracted from config.from below) handles the alias; self_email
        // handles the primary.
        let self_addr = if reply_all {
            Some(fetch_user_email(&c, &t).await?)
        } else {
            None
        };
        (orig, Some(t), self_addr, Some(c))
    };

    let self_email = self_email.as_deref();

    // Determine reply recipients
    let from_alias_email = config
        .from
        .as_ref()
        .and_then(|addrs| addrs.first())
        .map(|m| m.email.as_str());
    let mut reply_to = if reply_all {
        build_reply_all_recipients(
            &original,
            config.cc.as_deref(),
            config.remove.as_deref(),
            self_email,
            from_alias_email,
        )
    } else {
        Ok(ReplyRecipients {
            to: extract_reply_to_address(&original),
            cc: config.cc.clone(),
        })
    }?;

    // Append extra --to recipients
    if let Some(extra_to) = &config.extra_to {
        reply_to.to.extend(extra_to.iter().cloned());
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
    let refs = build_references_chain(&original);

    let envelope = ReplyEnvelope {
        to: &to,
        cc: non_empty_slice(&cc),
        bcc: non_empty_slice(&bcc),
        from: config.from.as_deref(),

        subject: &subject,
        threading: ThreadingHeaders {
            in_reply_to: &original.message_id,
            references: &refs,
        },
        body: &config.body,
        html: config.html,
    };

    // Fetch inline images for HTML replies only. In plain-text mode, inline
    // images are dropped entirely — matching Gmail web, which strips them from
    // both plain-text replies and plain-text forwards.
    let mut all_attachments = config.attachments;
    if let (true, Some(client), Some(token)) = (config.html, &client, &token) {
        let inline_parts: Vec<_> = original
            .parts
            .iter()
            .filter(|p| p.is_inline())
            .cloned()
            .collect();

        fetch_and_merge_original_parts(
            client,
            token,
            &config.message_id,
            &inline_parts,
            &mut all_attachments,
        )
        .await?;
    }

    let raw = create_reply_raw_message(&envelope, &original, &all_attachments)?;

    super::dispatch_raw_email(
        doc,
        matches,
        &raw,
        original.thread_id.as_deref(),
        token.as_deref(),
    )
    .await
}

// --- Data structures ---

#[derive(Debug)]
struct ReplyRecipients {
    to: Vec<Mailbox>,
    cc: Option<Vec<Mailbox>>,
}

struct ReplyEnvelope<'a> {
    to: &'a [Mailbox],
    cc: Option<&'a [Mailbox]>,
    bcc: Option<&'a [Mailbox]>,
    from: Option<&'a [Mailbox]>,
    subject: &'a str,
    threading: ThreadingHeaders<'a>,
    body: &'a str, // Always present: --body is required for replies
    html: bool,    // When true, body content is treated as HTML
}

pub(super) struct ReplyConfig {
    pub message_id: String,
    pub body: String,
    pub from: Option<Vec<Mailbox>>,
    pub extra_to: Option<Vec<Mailbox>>,
    pub cc: Option<Vec<Mailbox>>,
    pub bcc: Option<Vec<Mailbox>>,
    pub remove: Option<Vec<Mailbox>>,
    pub html: bool,
    pub attachments: Vec<Attachment>,
}

/// Fetch the authenticated user's primary email from the Gmail profile API.
/// Used in reply-all for self-dedup (excluding the user from recipients) and
/// self-reply detection (switching to original-To-based addressing).
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
        let body = resp
            .text()
            .await
            .unwrap_or_else(|_| "(error body unreadable)".to_string());
        return Err(super::build_api_error(
            status,
            &body,
            "Failed to fetch user profile",
        ));
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

fn extract_reply_to_address(original: &OriginalMessage) -> Vec<Mailbox> {
    match &original.reply_to {
        Some(reply_to) => reply_to.clone(),
        None => vec![original.from.clone()],
    }
}

fn build_reply_all_recipients(
    original: &OriginalMessage,
    extra_cc: Option<&[Mailbox]>,
    remove: Option<&[Mailbox]>,
    self_email: Option<&str>,
    from_alias: Option<&str>,
) -> Result<ReplyRecipients, GwsError> {
    let excluded = collect_excluded_emails(remove, self_email, from_alias);

    // When replying to your own message, the original sender (you) would be
    // excluded from To, leaving it empty. Gmail web handles this by using the
    // original To recipients as the reply targets instead, ignoring Reply-To.
    // (Gmail ignores Reply-To on self-sent messages — we approximate this by
    // checking the primary address and the current From alias.)
    let is_self_reply = [self_email, from_alias]
        .into_iter()
        .flatten()
        .any(|e| original.from.email.eq_ignore_ascii_case(e));

    let (to_candidates, mut cc_candidates) = if is_self_reply {
        // Self-reply: To = original To, CC = original CC
        let cc = original.cc.clone().unwrap_or_default();
        (original.to.clone(), cc)
    } else {
        // Normal reply: To = Reply-To or From, CC = original To + CC
        let mut cc = original.to.clone();
        if let Some(orig_cc) = &original.cc {
            cc.extend(orig_cc.iter().cloned());
        }
        (extract_reply_to_address(original), cc)
    };

    let mut to_emails = std::collections::HashSet::new();
    let to: Vec<Mailbox> = to_candidates
        .into_iter()
        .filter(|m| {
            let email = m.email_lowercase();
            if email.is_empty() || excluded.contains(&email) {
                return false;
            }
            to_emails.insert(email)
        })
        .collect();

    // Add extra CC if provided
    if let Some(extra) = extra_cc {
        cc_candidates.extend(extra.iter().cloned());
    }

    // Filter CC: remove To recipients, excluded addresses, and duplicates
    let mut seen = std::collections::HashSet::new();
    let cc: Vec<Mailbox> = cc_candidates
        .into_iter()
        .filter(|m| {
            let email = m.email_lowercase();
            !email.is_empty()
                && !to_emails.contains(&email)
                && !excluded.contains(&email)
                && seen.insert(email)
        })
        .collect();

    let cc = if cc.is_empty() { None } else { Some(cc) };

    Ok(ReplyRecipients { to, cc })
}

/// Deduplicate recipients across To, CC, and BCC fields.
///
/// Priority: To > CC > BCC. If an email appears in multiple fields,
/// it is kept only in the highest-priority field.
fn dedup_recipients(
    to: &[Mailbox],
    cc: Option<&[Mailbox]>,
    bcc: Option<&[Mailbox]>,
) -> (Vec<Mailbox>, Vec<Mailbox>, Vec<Mailbox>) {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut dedup = |mailboxes: &[Mailbox]| -> Vec<Mailbox> {
        mailboxes
            .iter()
            .filter(|m| {
                let email = m.email_lowercase();
                !email.is_empty() && seen.insert(email)
            })
            .cloned()
            .collect()
    };

    let to_out = dedup(to);
    let cc_out = dedup(cc.unwrap_or(&[]));
    let bcc_out = dedup(bcc.unwrap_or(&[]));

    (to_out, cc_out, bcc_out)
}

fn collect_excluded_emails(
    remove: Option<&[Mailbox]>,
    self_email: Option<&str>,
    from_alias: Option<&str>,
) -> std::collections::HashSet<String> {
    let mut excluded = std::collections::HashSet::new();

    if let Some(remove) = remove {
        excluded.extend(
            remove
                .iter()
                .map(|m| m.email_lowercase())
                .filter(|email| !email.is_empty()),
        );
    }

    // Exclude the user's own address and any --from alias
    for raw in [self_email, from_alias].into_iter().flatten() {
        let email = Mailbox::parse(raw).email_lowercase();
        if !email.is_empty() {
            excluded.insert(email);
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

fn create_reply_raw_message(
    envelope: &ReplyEnvelope,
    original: &OriginalMessage,
    attachments: &[Attachment],
) -> Result<String, GwsError> {
    let mb = mail_builder::MessageBuilder::new()
        .to(to_mb_address_list(envelope.to))
        .subject(envelope.subject);

    let mb = apply_optional_headers(mb, envelope.from, envelope.cc, envelope.bcc);
    let mb = set_threading_headers(mb, &envelope.threading);

    let (quoted, separator) = if envelope.html {
        (format_quoted_original_html(original), "<br>\r\n")
    } else {
        (format_quoted_original(original), "\r\n\r\n")
    };
    let body = format!("{}{}{}", envelope.body, separator, quoted);

    finalize_message(mb, body, envelope.html, attachments)
}

fn format_quoted_original(original: &OriginalMessage) -> String {
    let quoted_body: String = original
        .body_text
        .lines()
        .map(|line| format!("> {}", line))
        .collect::<Vec<_>>()
        .join("\r\n");

    let attribution = match &original.date {
        Some(date) => format!("On {}, {} wrote:", date, original.from),
        None => format!("{} wrote:", original.from),
    };
    format!("{}\r\n{}", attribution, quoted_body)
}

fn format_quoted_original_html(original: &OriginalMessage) -> String {
    let quoted_body = resolve_html_body(original);
    let sender = format_sender_for_attribution(&original.from);

    let attribution = match &original.date {
        Some(date) => {
            let formatted = format_date_for_attribution(date);
            format!("On {}, {} wrote:", formatted, sender)
        }
        None => format!("{} wrote:", sender),
    };

    format!(
        "<div class=\"gmail_quote gmail_quote_container\">\
           <div dir=\"ltr\" class=\"gmail_attr\">\
             {}<br>\
           </div>\
           <blockquote class=\"gmail_quote\" \
             style=\"margin:0 0 0 0.8ex;\
             border-left:1px solid rgb(204,204,204);\
             padding-left:1ex\">\
             <div dir=\"ltr\">{}</div>\
           </blockquote>\
         </div>",
        attribution, quoted_body,
    )
}

// --- Argument parsing ---

fn parse_reply_args(matches: &ArgMatches) -> Result<ReplyConfig, GwsError> {
    // try_get_one because +reply doesn't define --remove (only +reply-all does).
    // Explicit match distinguishes "arg not defined" from unexpected errors.
    let remove = match matches.try_get_one::<String>("remove") {
        Ok(val) => val
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(|s| Mailbox::parse_list(&s))
            .filter(|v| !v.is_empty()),
        Err(clap::parser::MatchesError::UnknownArgument { .. }) => None,
        Err(e) => {
            return Err(GwsError::Other(anyhow::anyhow!(
                "Unexpected error reading --remove argument: {e}"
            )))
        }
    };

    Ok(ReplyConfig {
        message_id: matches.get_one::<String>("message-id").unwrap().to_string(),
        body: matches.get_one::<String>("body").unwrap().to_string(),
        from: parse_optional_mailboxes(matches, "from"),
        extra_to: parse_optional_mailboxes(matches, "to"),
        cc: parse_optional_mailboxes(matches, "cc"),
        bcc: parse_optional_mailboxes(matches, "bcc"),
        remove,
        html: matches.get_flag("html"),
        attachments: parse_attachments(matches)?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{extract_header, strip_qp_soft_breaks};
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
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original body".to_string(),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = vec![Mailbox::parse("alice@example.com")];
        let envelope = ReplyEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Re: Hello",
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
            body: "My reply",
            html: false,
        };
        let raw = create_reply_raw_message(&envelope, &original, &[]).unwrap();

        let to_header = extract_header(&raw, "To").unwrap();
        assert!(to_header.contains("alice@example.com"));
        assert!(extract_header(&raw, "Subject")
            .unwrap()
            .contains("Re: Hello"));
        assert!(extract_header(&raw, "In-Reply-To")
            .unwrap()
            .contains("abc@example.com"));
        assert!(raw.contains("text/plain"));
        assert!(raw.contains("My reply"));
        assert!(raw.contains("> Original body"));
    }

    #[test]
    fn test_create_reply_raw_message_with_all_optional_headers() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original body".to_string(),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = vec![Mailbox::parse("alice@example.com")];
        let cc = vec![Mailbox::parse("carol@example.com")];
        let bcc = vec![Mailbox::parse("secret@example.com")];
        let from = Mailbox::parse_list("alias@example.com");
        let envelope = ReplyEnvelope {
            to: &to,
            cc: Some(&cc),
            bcc: Some(&bcc),
            from: Some(&from),
            subject: "Re: Hello",
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
            body: "Reply with all headers",
            html: false,
        };
        let raw = create_reply_raw_message(&envelope, &original, &[]).unwrap();

        assert!(extract_header(&raw, "Cc")
            .unwrap()
            .contains("carol@example.com"));
        assert!(extract_header(&raw, "Bcc")
            .unwrap()
            .contains("secret@example.com"));
        assert!(extract_header(&raw, "From")
            .unwrap()
            .contains("alias@example.com"));
    }

    #[test]
    fn test_build_reply_all_recipients() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![
                Mailbox::parse("bob@example.com"),
                Mailbox::parse("carol@example.com"),
            ],
            cc: Some(vec![Mailbox::parse("dave@example.com")]),
            subject: "Hello".to_string(),
            ..Default::default()
        };

        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        assert_eq!(recipients.to.len(), 1);
        assert_eq!(recipients.to[0].email, "alice@example.com");
        let cc = recipients.cc.unwrap();
        assert!(cc.iter().any(|m| m.email == "bob@example.com"));
        assert!(cc.iter().any(|m| m.email == "carol@example.com"));
        assert!(cc.iter().any(|m| m.email == "dave@example.com"));
        // Sender should not be in CC
        assert!(!cc.iter().any(|m| m.email == "alice@example.com"));
    }

    #[test]
    fn test_build_reply_all_with_remove() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![
                Mailbox::parse("bob@example.com"),
                Mailbox::parse("carol@example.com"),
            ],
            subject: "Hello".to_string(),
            ..Default::default()
        };

        let remove = Mailbox::parse_list("carol@example.com");
        let recipients =
            build_reply_all_recipients(&original, None, Some(&remove), None, None).unwrap();
        let cc = recipients.cc.unwrap();
        assert!(cc.iter().any(|m| m.email == "bob@example.com"));
        assert!(!cc.iter().any(|m| m.email == "carol@example.com"));
    }

    #[test]
    fn test_build_reply_all_remove_primary_returns_empty_to() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            ..Default::default()
        };

        let remove = Mailbox::parse_list("alice@example.com");
        let recipients =
            build_reply_all_recipients(&original, None, Some(&remove), None, None).unwrap();
        assert!(recipients.to.is_empty());
    }

    #[test]
    fn test_reply_all_excludes_from_alias_from_cc() {
        let original = OriginalMessage {
            from: Mailbox::parse("sender@example.com"),
            to: vec![
                Mailbox::parse("sales@example.com"),
                Mailbox::parse("bob@example.com"),
            ],
            cc: Some(vec![Mailbox::parse("carol@example.com")]),
            subject: "Hello".to_string(),
            ..Default::default()
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

        assert!(!cc.iter().any(|m| m.email == "sales@example.com"));
        assert!(cc.iter().any(|m| m.email == "bob@example.com"));
        assert!(cc.iter().any(|m| m.email == "carol@example.com"));
    }

    #[test]
    fn test_build_reply_all_from_alias_is_self_reply() {
        // When from_alias matches original.from, this is a self-reply.
        // To should be the original To recipients, not empty.
        let original = OriginalMessage {
            from: Mailbox::parse("sales@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            ..Default::default()
        };

        let recipients = build_reply_all_recipients(
            &original,
            None,
            None,
            Some("me@example.com"),
            Some("sales@example.com"),
        )
        .unwrap();
        assert_eq!(recipients.to.len(), 1);
        assert_eq!(recipients.to[0].email, "bob@example.com");
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
            .arg(Arg::new("html").long("html").action(ArgAction::SetTrue))
            .arg(
                Arg::new("attach")
                    .short('a')
                    .long("attach")
                    .action(ArgAction::Append),
            )
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("draft").long("draft").action(ArgAction::SetTrue));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_reply_args() {
        let matches = make_reply_matches(&["test", "--message-id", "abc123", "--body", "My reply"]);
        let config = parse_reply_args(&matches).unwrap();
        assert_eq!(config.message_id, "abc123");
        assert_eq!(config.body, "My reply");
        assert!(config.extra_to.is_none());
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
        assert_eq!(
            config.extra_to.as_ref().unwrap()[0].email,
            "dave@example.com"
        );
        assert_eq!(config.cc.as_ref().unwrap()[0].email, "extra@example.com");
        assert_eq!(config.bcc.as_ref().unwrap()[0].email, "secret@example.com");
        assert_eq!(
            config.remove.as_ref().unwrap()[0].email,
            "unwanted@example.com"
        );

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
        assert!(config.extra_to.is_none());
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
    }

    #[test]
    fn test_parse_reply_args_html_flag() {
        let matches = make_reply_matches(&[
            "test",
            "--message-id",
            "abc123",
            "--body",
            "<b>Bold</b>",
            "--html",
        ]);
        let config = parse_reply_args(&matches).unwrap();
        assert!(config.html);

        // Default is false
        let matches =
            make_reply_matches(&["test", "--message-id", "abc123", "--body", "Plain reply"]);
        let config = parse_reply_args(&matches).unwrap();
        assert!(!config.html);
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
            .arg(Arg::new("bcc").long("bcc"))
            .arg(Arg::new("html").long("html").action(ArgAction::SetTrue))
            .arg(
                Arg::new("attach")
                    .short('a')
                    .long("attach")
                    .action(ArgAction::Append),
            );
        let matches = cmd
            .try_get_matches_from(&["test", "--message-id", "abc", "--body", "hi"])
            .unwrap();
        let config = parse_reply_args(&matches).unwrap();
        assert!(config.remove.is_none());
    }

    #[test]
    fn test_extract_reply_to_address_falls_back_to_from() {
        let original = OriginalMessage {
            from: Mailbox::parse("Alice <alice@example.com>"),
            ..Default::default()
        };
        let addrs = extract_reply_to_address(&original);
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].email, "alice@example.com");
        assert_eq!(addrs[0].name.as_deref(), Some("Alice"));
    }

    #[test]
    fn test_extract_reply_to_address_prefers_reply_to() {
        let original = OriginalMessage {
            from: Mailbox::parse("Alice <alice@example.com>"),
            reply_to: Some(vec![Mailbox::parse("list@example.com")]),
            ..Default::default()
        };
        let addrs = extract_reply_to_address(&original);
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].email, "list@example.com");
    }

    #[test]
    fn test_remove_does_not_match_substring() {
        let original = OriginalMessage {
            from: Mailbox::parse("sender@example.com"),
            to: vec![
                Mailbox::parse("ann@example.com"),
                Mailbox::parse("joann@example.com"),
            ],
            ..Default::default()
        };
        let remove = Mailbox::parse_list("ann@example.com");
        let recipients =
            build_reply_all_recipients(&original, None, Some(&remove), None, None).unwrap();
        let cc = recipients.cc.unwrap();
        // joann@example.com should remain, ann@example.com should be removed
        assert_eq!(cc.len(), 1);
        assert_eq!(cc[0].email, "joann@example.com");
    }

    #[test]
    fn test_reply_all_uses_reply_to_for_to() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            reply_to: Some(vec![Mailbox::parse("list@example.com")]),
            to: vec![Mailbox::parse("bob@example.com")],
            ..Default::default()
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        assert_eq!(recipients.to[0].email, "list@example.com");
        let cc = recipients.cc.unwrap();
        assert!(cc.iter().any(|m| m.email == "bob@example.com"));
        // list@example.com is in To, should not duplicate in CC
        assert!(!cc.iter().any(|m| m.email == "list@example.com"));
    }

    #[test]
    fn test_sender_with_display_name_excluded_from_cc() {
        let original = OriginalMessage {
            from: Mailbox::parse("Alice <alice@example.com>"),
            to: vec![
                Mailbox::parse("alice@example.com"),
                Mailbox::parse("bob@example.com"),
            ],
            ..Default::default()
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        assert_eq!(recipients.to[0].email, "alice@example.com");
        let cc = recipients.cc.unwrap();
        assert_eq!(cc.len(), 1);
        assert_eq!(cc[0].email, "bob@example.com");
    }

    #[test]
    fn test_remove_with_display_name_format() {
        let original = OriginalMessage {
            from: Mailbox::parse("sender@example.com"),
            to: vec![
                Mailbox::parse("bob@example.com"),
                Mailbox::parse("carol@example.com"),
            ],
            ..Default::default()
        };
        let remove = Mailbox::parse_list("Carol <carol@example.com>");
        let recipients =
            build_reply_all_recipients(&original, None, Some(&remove), None, None).unwrap();
        let cc = recipients.cc.unwrap();
        assert_eq!(cc.len(), 1);
        assert_eq!(cc[0].email, "bob@example.com");
    }

    #[test]
    fn test_reply_all_with_extra_cc() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            ..Default::default()
        };
        let extra_cc = Mailbox::parse_list("extra@example.com");
        let recipients =
            build_reply_all_recipients(&original, Some(&extra_cc), None, None, None).unwrap();
        let cc = recipients.cc.unwrap();
        assert!(cc.iter().any(|m| m.email == "bob@example.com"));
        assert!(cc.iter().any(|m| m.email == "extra@example.com"));
    }

    #[test]
    fn test_reply_all_cc_none_when_all_filtered() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("alice@example.com")],
            ..Default::default()
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        assert!(recipients.cc.is_none());
    }

    #[test]
    fn test_case_insensitive_sender_exclusion() {
        let original = OriginalMessage {
            from: Mailbox::parse("Alice@Example.COM"),
            to: vec![
                Mailbox::parse("alice@example.com"),
                Mailbox::parse("bob@example.com"),
            ],
            ..Default::default()
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        let cc = recipients.cc.unwrap();
        assert_eq!(cc.len(), 1);
        assert_eq!(cc[0].email, "bob@example.com");
    }

    #[test]
    fn test_reply_all_multi_address_reply_to_deduplicates_cc() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            reply_to: Some(vec![
                Mailbox::parse("list@example.com"),
                Mailbox::parse("owner@example.com"),
            ]),
            to: vec![
                Mailbox::parse("bob@example.com"),
                Mailbox::parse("list@example.com"),
            ],
            cc: Some(vec![
                Mailbox::parse("owner@example.com"),
                Mailbox::parse("dave@example.com"),
            ]),
            ..Default::default()
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        assert_eq!(recipients.to.len(), 2);
        assert_eq!(recipients.to[0].email, "list@example.com");
        assert_eq!(recipients.to[1].email, "owner@example.com");
        let cc = recipients.cc.unwrap();
        assert!(cc.iter().any(|m| m.email == "bob@example.com"));
        assert!(cc.iter().any(|m| m.email == "dave@example.com"));
        assert!(!cc.iter().any(|m| m.email == "list@example.com"));
        assert!(!cc.iter().any(|m| m.email == "owner@example.com"));
    }

    #[test]
    fn test_reply_all_with_quoted_comma_display_name() {
        let original = OriginalMessage {
            from: Mailbox::parse("sender@example.com"),
            to: Mailbox::parse_list(r#""Doe, John" <john@example.com>, alice@example.com"#),
            ..Default::default()
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        let cc = recipients.cc.unwrap();
        assert!(cc.iter().any(|m| m.email == "john@example.com"));
        assert!(cc.iter().any(|m| m.email == "alice@example.com"));
    }

    #[test]
    fn test_remove_with_quoted_comma_display_name() {
        let original = OriginalMessage {
            from: Mailbox::parse("sender@example.com"),
            to: Mailbox::parse_list(r#""Doe, John" <john@example.com>, alice@example.com"#),
            ..Default::default()
        };
        let remove = Mailbox::parse_list("john@example.com");
        let recipients = build_reply_all_recipients(&original, None, Some(&remove), None, None);
        let cc = recipients.unwrap().cc.unwrap();
        assert!(!cc.iter().any(|m| m.email == "john@example.com"));
        assert!(cc.iter().any(|m| m.email == "alice@example.com"));
    }

    #[test]
    fn test_reply_all_excludes_self_email() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![
                Mailbox::parse("me@example.com"),
                Mailbox::parse("bob@example.com"),
            ],
            ..Default::default()
        };
        let recipients =
            build_reply_all_recipients(&original, None, None, Some("me@example.com"), None)
                .unwrap();
        let cc = recipients.cc.unwrap();
        assert!(cc.iter().any(|m| m.email == "bob@example.com"));
        assert!(!cc.iter().any(|m| m.email == "me@example.com"));
    }

    #[test]
    fn test_reply_all_excludes_self_case_insensitive() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![
                Mailbox::parse("Me@Example.COM"),
                Mailbox::parse("bob@example.com"),
            ],
            ..Default::default()
        };
        let recipients =
            build_reply_all_recipients(&original, None, None, Some("me@example.com"), None)
                .unwrap();
        let cc = recipients.cc.unwrap();
        assert!(cc.iter().any(|m| m.email == "bob@example.com"));
        assert!(!cc.iter().any(|m| m.email_lowercase() == "me@example.com"));
    }

    #[test]
    fn test_reply_all_deduplicates_cc() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            cc: Some(vec![
                Mailbox::parse("bob@example.com"),
                Mailbox::parse("carol@example.com"),
            ]),
            ..Default::default()
        };
        let recipients = build_reply_all_recipients(&original, None, None, None, None).unwrap();
        let cc = recipients.cc.unwrap();
        assert_eq!(
            cc.iter().filter(|m| m.email == "bob@example.com").count(),
            1
        );
        assert!(cc.iter().any(|m| m.email == "carol@example.com"));
    }

    // --- self-reply tests ---

    #[test]
    fn test_reply_all_to_own_message_puts_original_to_in_to() {
        let original = OriginalMessage {
            from: Mailbox::parse("me@example.com"),
            to: vec![
                Mailbox::parse("alice@example.com"),
                Mailbox::parse("bob@example.com"),
            ],
            cc: Some(vec![Mailbox::parse("carol@example.com")]),
            ..Default::default()
        };
        let recipients =
            build_reply_all_recipients(&original, None, None, Some("me@example.com"), None)
                .unwrap();
        // To should be the original To recipients, not the original sender
        assert_eq!(recipients.to.len(), 2);
        assert!(recipients.to.iter().any(|m| m.email == "alice@example.com"));
        assert!(recipients.to.iter().any(|m| m.email == "bob@example.com"));
        // CC should be the original CC
        let cc = recipients.cc.unwrap();
        assert_eq!(cc.len(), 1);
        assert!(cc.iter().any(|m| m.email == "carol@example.com"));
    }

    #[test]
    fn test_reply_all_to_own_message_detected_via_alias() {
        let original = OriginalMessage {
            from: Mailbox::parse("alias@work.com"),
            to: vec![Mailbox::parse("alice@example.com")],
            ..Default::default()
        };
        // self_email is primary, from_alias matches the original sender
        let recipients = build_reply_all_recipients(
            &original,
            None,
            None,
            Some("me@gmail.com"),
            Some("alias@work.com"),
        )
        .unwrap();
        assert_eq!(recipients.to.len(), 1);
        assert_eq!(recipients.to[0].email, "alice@example.com");
    }

    #[test]
    fn test_reply_all_to_own_message_excludes_self_from_original_to() {
        // You sent to yourself + Alice (e.g. a note-to-self CC'd to someone)
        let original = OriginalMessage {
            from: Mailbox::parse("me@example.com"),
            to: vec![
                Mailbox::parse("me@example.com"),
                Mailbox::parse("alice@example.com"),
            ],
            ..Default::default()
        };
        let recipients =
            build_reply_all_recipients(&original, None, None, Some("me@example.com"), None)
                .unwrap();
        // Self should still be excluded from To
        assert_eq!(recipients.to.len(), 1);
        assert_eq!(recipients.to[0].email, "alice@example.com");
    }

    #[test]
    fn test_reply_all_to_own_message_ignores_reply_to() {
        // Gmail web ignores Reply-To on self-sent messages. Verify that
        // self-reply uses original.to, not Reply-To.
        let original = OriginalMessage {
            from: Mailbox::parse("me@example.com"),
            to: vec![Mailbox::parse("alice@example.com")],
            reply_to: Some(vec![Mailbox::parse("list@example.com")]),
            ..Default::default()
        };
        let recipients =
            build_reply_all_recipients(&original, None, None, Some("me@example.com"), None)
                .unwrap();
        assert_eq!(recipients.to.len(), 1);
        assert_eq!(recipients.to[0].email, "alice@example.com");
        // No CC — Reply-To address should not appear anywhere
        assert!(recipients.cc.is_none());
    }

    // --- dedup_recipients tests ---

    #[test]
    fn test_dedup_no_overlap() {
        let to = vec![Mailbox::parse("alice@example.com")];
        let cc = vec![Mailbox::parse("bob@example.com")];
        let bcc = vec![Mailbox::parse("carol@example.com")];
        let (to_out, cc_out, bcc_out) = dedup_recipients(&to, Some(&cc), Some(&bcc));
        assert_eq!(to_out[0].email, "alice@example.com");
        assert_eq!(cc_out[0].email, "bob@example.com");
        assert_eq!(bcc_out[0].email, "carol@example.com");
    }

    #[test]
    fn test_dedup_to_wins_over_cc() {
        let to = vec![Mailbox::parse("alice@example.com")];
        let cc = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("bob@example.com"),
        ];
        let (to_out, cc_out, _) = dedup_recipients(&to, Some(&cc), None);
        assert_eq!(to_out[0].email, "alice@example.com");
        assert_eq!(cc_out.len(), 1);
        assert_eq!(cc_out[0].email, "bob@example.com");
    }

    #[test]
    fn test_dedup_to_wins_over_bcc() {
        let to = vec![Mailbox::parse("alice@example.com")];
        let bcc = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("carol@example.com"),
        ];
        let (to_out, _, bcc_out) = dedup_recipients(&to, None, Some(&bcc));
        assert_eq!(to_out[0].email, "alice@example.com");
        assert_eq!(bcc_out.len(), 1);
        assert_eq!(bcc_out[0].email, "carol@example.com");
    }

    #[test]
    fn test_dedup_cc_wins_over_bcc() {
        let to = vec![Mailbox::parse("alice@example.com")];
        let cc = vec![Mailbox::parse("bob@example.com")];
        let bcc = vec![
            Mailbox::parse("bob@example.com"),
            Mailbox::parse("carol@example.com"),
        ];
        let (_, cc_out, bcc_out) = dedup_recipients(&to, Some(&cc), Some(&bcc));
        assert_eq!(cc_out[0].email, "bob@example.com");
        assert_eq!(bcc_out.len(), 1);
        assert_eq!(bcc_out[0].email, "carol@example.com");
    }

    #[test]
    fn test_dedup_all_three_overlap() {
        let to = vec![Mailbox::parse("alice@example.com")];
        let cc = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("bob@example.com"),
        ];
        let bcc = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("bob@example.com"),
            Mailbox::parse("carol@example.com"),
        ];
        let (to_out, cc_out, bcc_out) = dedup_recipients(&to, Some(&cc), Some(&bcc));
        assert_eq!(to_out[0].email, "alice@example.com");
        assert_eq!(cc_out[0].email, "bob@example.com");
        assert_eq!(bcc_out[0].email, "carol@example.com");
    }

    #[test]
    fn test_dedup_case_insensitive() {
        let to = vec![Mailbox::parse("Alice@Example.COM")];
        let cc = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("bob@example.com"),
        ];
        let (to_out, cc_out, _) = dedup_recipients(&to, Some(&cc), None);
        assert_eq!(to_out[0].email, "Alice@Example.COM");
        assert_eq!(cc_out.len(), 1);
        assert_eq!(cc_out[0].email, "bob@example.com");
    }

    #[test]
    fn test_dedup_bcc_fully_overlaps_returns_empty() {
        let to = vec![Mailbox::parse("alice@example.com")];
        let cc = vec![Mailbox::parse("bob@example.com")];
        let bcc = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("bob@example.com"),
        ];
        let (_, _, bcc_out) = dedup_recipients(&to, Some(&cc), Some(&bcc));
        assert!(bcc_out.is_empty());
    }

    #[test]
    fn test_dedup_with_display_names() {
        let to = vec![Mailbox::parse("Alice <alice@example.com>")];
        let cc = vec![
            Mailbox::parse("alice@example.com"),
            Mailbox::parse("bob@example.com"),
        ];
        let (to_out, cc_out, _) = dedup_recipients(&to, Some(&cc), None);
        assert_eq!(to_out[0].email, "alice@example.com");
        assert_eq!(to_out[0].name.as_deref(), Some("Alice"));
        assert_eq!(cc_out.len(), 1);
        assert_eq!(cc_out[0].email, "bob@example.com");
    }

    #[test]
    fn test_dedup_intro_pattern() {
        let to = vec![Mailbox::parse("bob@example.com")];
        let cc = vec![Mailbox::parse("bob@example.com")];
        let bcc = vec![Mailbox::parse("alice@example.com")];
        let (to_out, cc_out, bcc_out) = dedup_recipients(&to, Some(&cc), Some(&bcc));
        assert_eq!(to_out[0].email, "bob@example.com");
        assert!(cc_out.is_empty());
        assert_eq!(bcc_out[0].email, "alice@example.com");
    }

    #[test]
    fn test_dedup_simple_reply_no_cc_bcc() {
        let to = vec![Mailbox::parse("alice@example.com")];
        let (to_out, cc_out, bcc_out) = dedup_recipients(&to, None, None);
        assert_eq!(to_out.len(), 1);
        assert_eq!(to_out[0].email, "alice@example.com");
        assert!(cc_out.is_empty());
        assert!(bcc_out.is_empty());
    }

    // --- format_quoted_original (plain text) ---

    #[test]
    fn test_format_quoted_original() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Line one\nLine two\nLine three".to_string(),
            ..Default::default()
        };
        let quoted = format_quoted_original(&original);
        assert!(quoted.contains("On Mon, 1 Jan 2026 00:00:00 +0000, alice@example.com wrote:"));
        assert!(quoted.contains("> Line one"));
        assert!(quoted.contains("> Line two"));
        assert!(quoted.contains("> Line three"));
    }

    #[test]
    fn test_format_quoted_original_empty_body() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            date: Some("Mon, 1 Jan 2026".to_string()),
            ..Default::default()
        };
        let quoted = format_quoted_original(&original);
        assert!(quoted.contains("alice@example.com wrote:"));
        // Empty body produces no quoted lines
        assert!(quoted.ends_with("wrote:\r\n"));
    }

    #[test]
    fn test_format_quoted_original_missing_date() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            date: None,
            body_text: "Hello".to_string(),
            ..Default::default()
        };
        let quoted = format_quoted_original(&original);
        assert!(quoted.starts_with("alice@example.com wrote:"));
        assert!(!quoted.contains("On "));
        assert!(quoted.contains("> Hello"));
    }

    // --- end-to-end --to behavioral tests ---

    #[test]
    fn test_extra_to_appears_in_raw_message() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("me@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original".to_string(),
            ..Default::default()
        };

        let mut to = extract_reply_to_address(&original);
        to.push(Mailbox::parse("dave@example.com"));

        let (to, cc, bcc) = dedup_recipients(&to, None, None);

        let refs = build_references_chain(&original);
        let envelope = ReplyEnvelope {
            to: &to,
            cc: non_empty_slice(&cc),
            bcc: non_empty_slice(&bcc),
            from: None,
            subject: "Re: Hello",
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
            body: "Adding Dave",
            html: false,
        };
        let raw = create_reply_raw_message(&envelope, &original, &[]).unwrap();

        let to_header = extract_header(&raw, "To").unwrap();
        assert!(to_header.contains("alice@example.com"));
        assert!(to_header.contains("dave@example.com"));
    }

    #[test]
    fn test_intro_pattern_raw_message() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("me@example.com")],
            cc: Some(vec![Mailbox::parse("bob@example.com")]),
            subject: "Intro".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Meet Bob".to_string(),
            ..Default::default()
        };

        // build_reply_all_recipients with --remove alice, self=me
        let remove = Mailbox::parse_list("alice@example.com");
        let recipients = build_reply_all_recipients(
            &original,
            None,
            Some(&remove),
            Some("me@example.com"),
            None,
        )
        .unwrap();

        // To is empty (alice removed)
        assert!(recipients.to.is_empty());

        // Append --to bob
        let to = vec![Mailbox::parse("bob@example.com")];

        // Dedup with --bcc alice
        let bcc = vec![Mailbox::parse("alice@example.com")];
        let (to, cc, bcc) = dedup_recipients(&to, recipients.cc.as_deref(), Some(&bcc));

        let refs = build_references_chain(&original);
        let envelope = ReplyEnvelope {
            to: &to,
            cc: non_empty_slice(&cc),
            bcc: non_empty_slice(&bcc),
            from: None,
            subject: "Re: Intro",
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
            body: "Hi Bob, nice to meet you!",
            html: false,
        };
        let raw = create_reply_raw_message(&envelope, &original, &[]).unwrap();

        let to_header = extract_header(&raw, "To").unwrap();
        assert!(to_header.contains("bob@example.com"));
        assert!(extract_header(&raw, "Bcc")
            .unwrap()
            .contains("alice@example.com"));
        assert!(raw.contains("Hi Bob, nice to meet you!"));
    }

    // --- HTML mode tests ---

    #[test]
    fn test_format_quoted_original_html_with_html_body() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            date: Some("Mon, 1 Jan 2026".to_string()),
            body_text: "plain fallback".to_string(),
            body_html: Some("<p>Rich <b>content</b></p>".to_string()),
            ..Default::default()
        };
        let html = format_quoted_original_html(&original);
        assert!(html.contains("gmail_quote"));
        assert!(html.contains("<blockquote"));
        assert!(html.contains("<p>Rich <b>content</b></p>"));
        assert!(!html.contains("plain fallback"));
        assert!(
            html.contains("<a href=\"mailto:alice%40example%2Ecom\">alice@example.com</a> wrote:")
        );
    }

    #[test]
    fn test_format_quoted_original_html_fallback_plain_text() {
        let original = OriginalMessage {
            from: Mailbox::parse("alice@example.com"),
            date: Some("Mon, 1 Jan 2026".to_string()),
            body_text: "Line one & <stuff>\nLine two".to_string(),
            ..Default::default()
        };
        let html = format_quoted_original_html(&original);
        assert!(html.contains("gmail_quote"));
        assert!(html.contains("<blockquote"));
        assert!(html.contains("Line one &amp; &lt;stuff&gt;<br>"));
        assert!(html.contains("Line two"));
    }

    #[test]
    fn test_format_quoted_original_html_escapes_metadata() {
        let original = OriginalMessage {
            from: Mailbox::parse("O'Brien & Associates <ob@example.com>"),
            date: Some("Jan 1 <2026>".to_string()),
            body_text: "text".to_string(),
            ..Default::default()
        };
        let html = format_quoted_original_html(&original);
        assert!(html.contains("O&#39;Brien &amp; Associates"));
        assert!(html.contains("&lt;<a href=\"mailto:ob%40example%2Ecom\">ob@example.com</a>&gt;"));
        assert!(html.contains("Jan 1 &lt;2026&gt;"));
    }

    #[test]
    fn test_create_reply_raw_message_html() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original body".to_string(),
            body_html: Some("<p>Original</p>".to_string()),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = vec![Mailbox::parse("alice@example.com")];
        let envelope = ReplyEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Re: Hello",
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
            body: "<p>My HTML reply</p>",
            html: true,
        };
        let raw = create_reply_raw_message(&envelope, &original, &[]).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(decoded.contains("text/html"));
        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("alice@example.com"));
        assert!(decoded.contains("<p>My HTML reply</p>"));
        assert!(decoded.contains("gmail_quote"));
        assert!(decoded.contains("<p>Original</p>"));
    }

    #[test]
    fn test_create_reply_raw_message_with_attachment() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Hello".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "Original body".to_string(),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = vec![Mailbox::parse("alice@example.com")];
        let envelope = ReplyEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Re: Hello",
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
            body: "See attached notes",
            html: false,
        };
        let attachments = vec![Attachment {
            filename: "notes.txt".to_string(),
            content_type: "text/plain".to_string(),
            data: b"some notes".to_vec(),
            content_id: None,
        }];
        let raw = create_reply_raw_message(&envelope, &original, &attachments).unwrap();

        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("notes.txt"));
        assert!(raw.contains("See attached notes"));
        assert!(raw.contains("> Original body"));
    }

    #[test]
    fn test_create_reply_raw_message_html_with_inline_image() {
        let original = OriginalMessage {
            thread_id: Some("t1".to_string()),
            message_id: "abc@example.com".to_string(),
            from: Mailbox::parse("alice@example.com"),
            to: vec![Mailbox::parse("bob@example.com")],
            subject: "Photo".to_string(),
            date: Some("Mon, 1 Jan 2026 00:00:00 +0000".to_string()),
            body_text: "See photo".to_string(),
            body_html: Some("<p>See <img src=\"cid:photo@example.com\"></p>".to_string()),
            ..Default::default()
        };

        let refs = build_references_chain(&original);
        let to = vec![Mailbox::parse("alice@example.com")];
        let envelope = ReplyEnvelope {
            to: &to,
            cc: None,
            bcc: None,
            from: None,
            subject: "Re: Photo",
            threading: ThreadingHeaders {
                in_reply_to: &original.message_id,
                references: &refs,
            },
            body: "Nice photo!",
            html: true,
        };
        let attachments = vec![Attachment {
            filename: "photo.png".to_string(),
            content_type: "image/png".to_string(),
            data: vec![0x89, 0x50],
            content_id: Some("photo@example.com".to_string()),
        }];
        let raw = create_reply_raw_message(&envelope, &original, &attachments).unwrap();

        // Should produce multipart/related for inline image in HTML reply
        assert!(raw.contains("multipart/related"));
        assert!(raw.contains("Content-ID: <photo@example.com>"));
        assert!(!raw.contains("multipart/mixed"));
    }
}

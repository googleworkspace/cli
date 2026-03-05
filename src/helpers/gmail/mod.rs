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

use super::Helper;
pub mod forward;
pub mod reply;
pub mod send;
pub mod triage;
pub mod watch;

use forward::handle_forward;
use reply::handle_reply;
use send::handle_send;
use triage::handle_triage;
use watch::handle_watch;

pub(super) use crate::auth;
pub(super) use crate::error::GwsError;
pub(super) use crate::executor;
pub(super) use anyhow::Context;
pub(super) use base64::{engine::general_purpose::URL_SAFE, Engine as _};
pub(super) use clap::{Arg, ArgAction, ArgMatches, Command};
pub(super) use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct GmailHelper;

pub(super) const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";
pub(super) const PUBSUB_SCOPE: &str = "https://www.googleapis.com/auth/pubsub";

/// Shared helper: base64-encode a raw RFC 2822 message and send it via
/// `users.messages.send`, keeping it in the given thread.
pub(super) async fn send_raw_email(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
    raw_message: &str,
    thread_id: &str,
) -> Result<(), GwsError> {
    let encoded = URL_SAFE.encode(raw_message);
    let body = json!({
        "raw": encoded,
        "threadId": thread_id,
    });
    let body_str = body.to_string();

    let send_method = reply::resolve_send_method(doc)?;
    let params = json!({ "userId": "me" });
    let params_str = params.to_string();

    let scopes: Vec<&str> = send_method.scopes.iter().map(|s| s.as_str()).collect();
    let (token, auth_method) = match auth::get_token(&scopes, None).await {
        Ok(t) => (Some(t), executor::AuthMethod::OAuth),
        Err(_) => (None, executor::AuthMethod::None),
    };

    let pagination = executor::PaginationConfig {
        page_all: false,
        page_limit: 10,
        page_delay_ms: 100,
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

impl Helper for GmailHelper {
    /// Injects helper subcommands (`+send`, `+watch`) into the main CLI command.
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+send")
                .about("[Helper] Send an email")
                .arg(
                    Arg::new("to")
                        .long("to")
                        .help("Recipient email address")
                        .required(true)
                        .value_name("EMAIL"),
                )
                .arg(
                    Arg::new("subject")
                        .long("subject")
                        .help("Email subject")
                        .required(true)
                        .value_name("SUBJECT"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .help("Email body (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi Alice!'

TIPS:
  Handles RFC 2822 formatting and base64 encoding automatically.
  For HTML bodies, attachments, or CC/BCC, use the raw API instead:
    gws gmail users messages send --json '...' ",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+triage")
                .about("[Helper] Show unread inbox summary (sender, subject, date)")
                .arg(
                    Arg::new("max")
                        .long("max")
                        .help("Maximum messages to show (default: 20)")
                        .default_value("20")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("query")
                        .long("query")
                        .help("Gmail search query (default: is:unread)")
                        .value_name("QUERY"),
                )
                .arg(
                    Arg::new("labels")
                        .long("labels")
                        .help("Include label names in output")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +triage
  gws gmail +triage --max 5 --query 'from:boss'
  gws gmail +triage --format json | jq '.[].subject'
  gws gmail +triage --labels

TIPS:
  Read-only — never modifies your mailbox.
  Defaults to table output format.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+reply")
                .about("[Helper] Reply to a message (handles threading automatically)")
                .arg(
                    Arg::new("message-id")
                        .long("message-id")
                        .help("Gmail message ID to reply to")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .help("Reply body (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("cc")
                        .long("cc")
                        .help("Additional CC recipients (comma-separated)")
                        .value_name("EMAILS"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Thanks, got it!'
  gws gmail +reply --message-id 18f1a2b3c4d --body 'Looping in Carol' --cc carol@example.com

TIPS:
  Automatically sets In-Reply-To, References, and threadId headers.
  Quotes the original message in the reply body.
  For reply-all, use +reply-all instead.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+reply-all")
                .about("[Helper] Reply-all to a message (handles threading automatically)")
                .arg(
                    Arg::new("message-id")
                        .long("message-id")
                        .help("Gmail message ID to reply to")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .help("Reply body (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("cc")
                        .long("cc")
                        .help("Additional CC recipients (comma-separated)")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("remove")
                        .long("remove")
                        .help("Remove recipients from the reply (comma-separated emails)")
                        .value_name("EMAILS"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Sounds good to me!'
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Updated' --remove bob@example.com
  gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Adding Eve' --cc eve@example.com

TIPS:
  Replies to the sender and all original To/CC recipients.
  Use --remove to drop recipients from the thread.
  Use --cc to add new recipients.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+forward")
                .about("[Helper] Forward a message to new recipients")
                .arg(
                    Arg::new("message-id")
                        .long("message-id")
                        .help("Gmail message ID to forward")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("to")
                        .long("to")
                        .help("Recipient email address(es), comma-separated")
                        .required(true)
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("cc")
                        .long("cc")
                        .help("CC recipients (comma-separated)")
                        .value_name("EMAILS"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .help("Optional note to include above the forwarded message")
                        .value_name("TEXT"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --body 'FYI see below'
  gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --cc eve@example.com

TIPS:
  Includes the original message with sender, date, subject, and recipients.
  Keeps the message in the same thread.",
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+watch")
                .about("[Helper] Watch for new emails and stream them as NDJSON")
                .arg(
                    Arg::new("project")
                        .long("project")
                        .help("GCP project ID for Pub/Sub resources")
                        .value_name("PROJECT"),
                )
                .arg(
                    Arg::new("subscription")
                        .long("subscription")
                        .help("Existing Pub/Sub subscription name (skip setup)")
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("topic")
                        .long("topic")
                        .help("Existing Pub/Sub topic with Gmail push permission already granted")
                        .value_name("TOPIC"),
                )
                .arg(
                    Arg::new("label-ids")
                        .long("label-ids")
                        .help("Comma-separated Gmail label IDs to filter (e.g., INBOX,UNREAD)")
                        .value_name("LABELS"),
                )
                .arg(
                    Arg::new("max-messages")
                        .long("max-messages")
                        .help("Max messages per pull batch")
                        .value_name("N")
                        .default_value("10"),
                )
                .arg(
                    Arg::new("poll-interval")
                        .long("poll-interval")
                        .help("Seconds between pulls")
                        .value_name("SECS")
                        .default_value("5"),
                )
                .arg(
                    Arg::new("msg-format")
                        .long("msg-format")
                        .help("Gmail message format: full, metadata, minimal, raw")
                        .value_name("FORMAT")
                        .value_parser(["full", "metadata", "minimal", "raw"])
                        .default_value("full"),
                )
                .arg(
                    Arg::new("once")
                        .long("once")
                        .help("Pull once and exit")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("cleanup")
                        .long("cleanup")
                        .help("Delete created Pub/Sub resources on exit")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("output-dir")
                        .long("output-dir")
                        .help("Write each message to a separate JSON file in this directory")
                        .value_name("DIR"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws gmail +watch --project my-gcp-project
  gws gmail +watch --project my-project --label-ids INBOX --once
  gws gmail +watch --subscription projects/p/subscriptions/my-sub
  gws gmail +watch --project my-project --cleanup --output-dir ./emails

TIPS:
  Gmail watch expires after 7 days — re-run to renew.
  Without --cleanup, Pub/Sub resources persist for reconnection.
  Press Ctrl-C to stop gracefully.",
                ),
        );

        cmd
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(matches) = matches.subcommand_matches("+send") {
                handle_send(doc, matches).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+reply") {
                handle_reply(doc, matches, false).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+reply-all") {
                handle_reply(doc, matches, true).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+forward") {
                handle_forward(doc, matches).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+triage") {
                handle_triage(matches).await?;
                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+watch") {
                handle_watch(matches, sanitize_config).await?;
                return Ok(true);
            }

            Ok(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_commands() {
        let helper = GmailHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();

        // No scopes granted -> defaults to showing all
        let cmd = helper.inject_commands(cmd, &doc);
        let subcommands: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subcommands.contains(&"+watch"));
        assert!(subcommands.contains(&"+send"));
        assert!(subcommands.contains(&"+reply"));
        assert!(subcommands.contains(&"+reply-all"));
        assert!(subcommands.contains(&"+forward"));
    }
}

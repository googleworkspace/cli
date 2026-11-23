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
use std::io::{self, Write};

/// How many messages to fetch at once when reading several search results.
const READ_CONCURRENCY: usize = 5;

/// Separator printed between messages when a query returns several.
const MESSAGE_SEPARATOR: &str = "========================================";

/// Rendering options shared by the `--id` and query modes.
#[derive(Clone, Copy)]
struct RenderOptions {
    show_headers: bool,
    use_html: bool,
}

/// An attachment or inline part, as reported by `+read` (metadata only).
#[derive(Serialize, Debug, PartialEq)]
struct AttachmentSummary {
    filename: String,
    content_type: String,
    size: u64,
    inline: bool,
}

/// JSON shape of one message printed by `+read`.
#[derive(Serialize)]
struct ReadOutput<'a> {
    id: &'a str,
    #[serde(flatten)]
    message: &'a OriginalMessage,
    attachments: Vec<AttachmentSummary>,
}

impl<'a> ReadOutput<'a> {
    fn new(id: &'a str, message: &'a OriginalMessage) -> Self {
        let attachments = message
            .parts
            .iter()
            .map(|p| AttachmentSummary {
                filename: p.filename.clone(),
                content_type: p.content_type.clone(),
                size: p.size,
                inline: p.is_inline(),
            })
            .collect();
        Self {
            id,
            message,
            attachments,
        }
    }
}

/// Handle the `+read` subcommand.
pub(super) async fn handle_read(
    _doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let dry_run = matches.get_flag("dry-run");
    let json = matches.get_one::<String>("format").map(String::as_str) == Some("json");
    let use_html = matches.get_flag("html");

    if let Some(message_id) = matches.get_one::<String>("id") {
        let original = if dry_run {
            OriginalMessage::dry_run_placeholder(message_id)
        } else {
            let t = read_token().await?;
            let client = crate::client::build_client()?;
            fetch_message_metadata(&client, &t, message_id).await?
        };
        let opts = RenderOptions {
            show_headers: matches.get_flag("headers"),
            use_html,
        };
        let output = if json {
            render_json_one(message_id, &original)?
        } else {
            render_text(&original, opts)
        };
        writeln!(io::stdout().lock(), "{output}").context("Failed to write message")?;
        return Ok(());
    }

    // Query mode: the clap ArgGroup guarantees `query` is present here.
    let raw_query = matches
        .get_one::<String>("query")
        .ok_or_else(|| GwsError::Validation("Provide a search query or --id".to_string()))?;
    let query = validate_search_query(raw_query)?;
    let limit = matches.get_one::<u32>("limit").copied().unwrap_or(1);
    // With a query the headers are the point: always show them.
    let opts = RenderOptions {
        show_headers: true,
        use_html,
    };

    let messages: Vec<(String, OriginalMessage)> = if dry_run {
        vec![(
            "dry-run".to_string(),
            OriginalMessage::dry_run_placeholder("dry-run"),
        )]
    } else {
        let t = read_token().await?;
        let client = crate::client::build_client()?;
        let ids = search_message_ids(&client, &t, query, limit).await?;
        fetch_messages(&client, &t, ids).await?
    };

    if messages.is_empty() && !json {
        eprintln!(
            "No messages match: {}",
            sanitize_for_terminal(&query.replace(['\r', '\n'], " "))
        );
        return Ok(());
    }

    let output = if json {
        render_json_many(&messages)?
    } else {
        render_text_many(&messages, opts)
    };
    writeln!(io::stdout().lock(), "{output}").context("Failed to write messages")?;
    Ok(())
}

async fn read_token() -> Result<String, GwsError> {
    auth::get_token(&[GMAIL_READONLY_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))
}

/// Fetch and parse several messages concurrently, preserving `ids` order.
async fn fetch_messages(
    client: &reqwest::Client,
    token: &str,
    ids: Vec<String>,
) -> Result<Vec<(String, OriginalMessage)>, GwsError> {
    use futures_util::stream::{self, StreamExt};

    stream::iter(ids)
        .map(|id| async move {
            let original = fetch_message_metadata(client, token, &id).await?;
            Ok::<_, GwsError>((id, original))
        })
        .buffered(READ_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect()
}

fn render_json_one(id: &str, original: &OriginalMessage) -> Result<String, GwsError> {
    serde_json::to_string_pretty(&ReadOutput::new(id, original))
        .context("Failed to serialize message to JSON")
        .map_err(GwsError::from)
}

fn render_json_many(messages: &[(String, OriginalMessage)]) -> Result<String, GwsError> {
    let outputs: Vec<ReadOutput<'_>> = messages
        .iter()
        .map(|(id, m)| ReadOutput::new(id, m))
        .collect();
    serde_json::to_string_pretty(&outputs)
        .context("Failed to serialize messages to JSON")
        .map_err(GwsError::from)
}

fn render_text_many(messages: &[(String, OriginalMessage)], opts: RenderOptions) -> String {
    messages
        .iter()
        .map(|(_, m)| render_text(m, opts))
        .collect::<Vec<_>>()
        .join(&format!("\n{MESSAGE_SEPARATOR}\n"))
}

/// Render one message as plain text: optional headers and attachment list,
/// a `---` separator, then the body. All sender-controlled text is sanitized
/// for the terminal.
fn render_text(original: &OriginalMessage, opts: RenderOptions) -> String {
    let mut out = String::new();

    if opts.show_headers {
        let from_str = original.from.to_string();
        let to_str = format_mailbox_list(&original.to);
        let cc_str = original
            .cc
            .as_ref()
            .map(|cc| format_mailbox_list(cc))
            .unwrap_or_default();

        let headers_to_show: [(&str, &str); 5] = [
            ("From", &from_str),
            ("To", &to_str),
            ("Cc", &cc_str),
            ("Date", original.date.as_deref().unwrap_or_default()),
            ("Subject", &original.subject),
        ];
        for (name, value) in headers_to_show {
            if value.is_empty() {
                continue;
            }
            out.push_str(&format!("{name}: {}\n", single_line(value)));
        }

        let attachments: Vec<&OriginalPart> =
            original.parts.iter().filter(|p| !p.is_inline()).collect();
        if !attachments.is_empty() {
            out.push_str("Attachments:\n");
            for part in attachments {
                out.push_str(&format!(
                    "  - {} ({}, {})\n",
                    single_line(&part.filename),
                    format_size(part.size),
                    single_line(&part.content_type)
                ));
            }
        }
        out.push_str("---\n");
    }

    let body = if opts.use_html {
        original
            .body_html
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&original.body_text)
    } else {
        &original.body_text
    };
    out.push_str(&sanitize_for_terminal(body));
    out
}

/// Collapse a header value to one line (prevents header spoofing in the
/// output) and strip terminal control sequences.
fn single_line(value: &str) -> String {
    sanitize_for_terminal(&value.replace(['\r', '\n'], " "))
}

/// Human-readable byte size: `512 B`, `12.3 KB`, `4.5 MB`.
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64 / 1024.0;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

/// Format a slice of Mailbox as a displayable comma-separated string.
fn format_mailbox_list(mailboxes: &[Mailbox]) -> String {
    mailboxes
        .iter()
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

use crate::output::sanitize_for_terminal;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_message() -> OriginalMessage {
        OriginalMessage {
            from: Mailbox::parse("Alice <alice@example.com>"),
            to: Mailbox::parse_list("bob@example.com"),
            cc: Some(Mailbox::parse_list("carol@example.com")),
            subject: "Hello\r\nBcc: evil@example.com".to_string(),
            date: Some("Mon, 23 Nov 2026 10:00:00 +0000".to_string()),
            body_text: "Line one\nLine two \x1b[31mred".to_string(),
            body_html: Some("<p>Line one</p>".to_string()),
            parts: vec![
                OriginalPart {
                    filename: "report.pdf".to_string(),
                    content_type: "application/pdf".to_string(),
                    size: 12_650,
                    attachment_id: "att1".to_string(),
                    content_id: None,
                },
                OriginalPart {
                    filename: "logo.png".to_string(),
                    content_type: "image/png".to_string(),
                    size: 300,
                    attachment_id: "att2".to_string(),
                    content_id: Some("logo".to_string()),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_render_text_with_headers_and_attachments() {
        let text = render_text(
            &sample_message(),
            RenderOptions {
                show_headers: true,
                use_html: false,
            },
        );
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("From: "));
        assert!(lines[0].contains("alice@example.com"));
        assert!(lines[1].starts_with("To: "));
        assert!(lines[2].starts_with("Cc: "));
        assert_eq!(lines[3], "Date: Mon, 23 Nov 2026 10:00:00 +0000");
        // CR/LF in a header cannot forge a new header line.
        assert_eq!(lines[4], "Subject: Hello  Bcc: evil@example.com");
        assert_eq!(lines[5], "Attachments:");
        assert_eq!(lines[6], "  - report.pdf (12.4 KB, application/pdf)");
        // Inline images are not listed as attachments.
        assert_eq!(lines[7], "---");
        assert_eq!(lines[8], "Line one");
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn test_render_text_body_only() {
        let text = render_text(
            &sample_message(),
            RenderOptions {
                show_headers: false,
                use_html: false,
            },
        );
        assert!(text.starts_with("Line one\nLine two"));
        assert!(!text.contains("From:"));
    }

    #[test]
    fn test_render_text_html_body() {
        let text = render_text(
            &sample_message(),
            RenderOptions {
                show_headers: false,
                use_html: true,
            },
        );
        assert_eq!(text, "<p>Line one</p>");

        let mut no_html = sample_message();
        no_html.body_html = Some("  ".to_string());
        let text = render_text(
            &no_html,
            RenderOptions {
                show_headers: false,
                use_html: true,
            },
        );
        assert!(text.starts_with("Line one"));
    }

    #[test]
    fn test_render_text_skips_empty_headers() {
        let msg = OriginalMessage {
            from: Mailbox::parse("a@example.com"),
            body_text: "x".to_string(),
            ..Default::default()
        };
        let text = render_text(
            &msg,
            RenderOptions {
                show_headers: true,
                use_html: false,
            },
        );
        assert_eq!(text, "From: a@example.com\n---\nx");
    }

    #[test]
    fn test_render_text_many_separates_messages() {
        let msgs = vec![
            ("1".to_string(), sample_message()),
            ("2".to_string(), sample_message()),
        ];
        let text = render_text_many(
            &msgs,
            RenderOptions {
                show_headers: true,
                use_html: false,
            },
        );
        assert_eq!(text.matches(MESSAGE_SEPARATOR).count(), 1);
        assert_eq!(text.matches("From: ").count(), 2);
    }

    #[test]
    fn test_render_json_one_includes_id_and_attachments() {
        let msg = sample_message();
        let json: Value = serde_json::from_str(&render_json_one("abc", &msg).unwrap()).unwrap();
        assert_eq!(json["id"], "abc");
        assert_eq!(json["subject"], "Hello\r\nBcc: evil@example.com");
        assert_eq!(json["attachments"][0]["filename"], "report.pdf");
        assert_eq!(json["attachments"][0]["size"], 12_650);
        assert_eq!(json["attachments"][0]["inline"], false);
        assert_eq!(json["attachments"][1]["inline"], true);
        assert!(json.get("parts").is_none());
    }

    #[test]
    fn test_render_json_many_is_array() {
        let msgs = vec![("1".to_string(), sample_message())];
        let json: Value = serde_json::from_str(&render_json_many(&msgs).unwrap()).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["id"], "1");
        let empty: Value = serde_json::from_str(&render_json_many(&[]).unwrap()).unwrap();
        assert_eq!(empty, json!([]));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
        assert_eq!(format_size(u64::MAX), "16777216.0 TB");
    }

    fn parse_read(args: &[&str]) -> Result<ArgMatches, clap::Error> {
        let cmd = GmailHelper.inject_commands(
            Command::new("gmail"),
            &crate::discovery::RestDescription::default(),
        );
        let mut argv = vec!["gmail", "+read"];
        argv.extend_from_slice(args);
        cmd.try_get_matches_from(argv)
            .map(|m| m.subcommand_matches("+read").unwrap().clone())
    }

    #[test]
    fn test_read_args_query_or_id() {
        let m = parse_read(&["from:a@example.com"]).unwrap();
        assert_eq!(m.get_one::<String>("query").unwrap(), "from:a@example.com");
        assert_eq!(*m.get_one::<u32>("limit").unwrap(), 1);

        let m = parse_read(&["from:a@example.com", "--limit", "3"]).unwrap();
        assert_eq!(*m.get_one::<u32>("limit").unwrap(), 3);

        let m = parse_read(&["--id", "18f"]).unwrap();
        assert_eq!(m.get_one::<String>("id").unwrap(), "18f");

        // Neither, both, and out-of-range limits are rejected.
        assert!(parse_read(&[]).is_err());
        assert!(parse_read(&["q", "--id", "18f"]).is_err());
        assert!(parse_read(&["--id", "18f", "--limit", "2"]).is_err());
        assert!(parse_read(&["q", "--limit", "0"]).is_err());
        assert!(parse_read(&["q", "--limit", "51"]).is_err());
    }

    #[test]
    fn test_sanitize_for_terminal() {
        let malicious = "Subject: \x1b]0;MALICIOUS\x07Hello\nWorld\r\t";
        let sanitized = sanitize_for_terminal(malicious);
        // ANSI escape sequences (control chars) should be removed
        assert!(!sanitized.contains('\x1b'));
        assert!(!sanitized.contains('\x07'));
        // CR is also stripped (can be abused for terminal overwrite attacks)
        assert!(!sanitized.contains('\r'));
        // Newline and tab should be preserved
        assert!(sanitized.contains("Hello"));
        assert!(sanitized.contains('\n'));
        assert!(sanitized.contains('\t'));
    }

    #[test]
    fn test_format_mailbox_list_empty() {
        assert_eq!(format_mailbox_list(&[]), "");
    }

    #[test]
    fn test_format_mailbox_list_single() {
        let mailboxes = Mailbox::parse_list("alice@example.com");
        let result = format_mailbox_list(&mailboxes);
        assert!(result.contains("alice@example.com"));
    }

    #[test]
    fn test_format_mailbox_list_multiple() {
        let mailboxes = Mailbox::parse_list("alice@example.com, Bob <bob@example.com>");
        let result = format_mailbox_list(&mailboxes);
        assert!(result.contains("alice@example.com"));
        assert!(result.contains("bob@example.com"));
    }
}

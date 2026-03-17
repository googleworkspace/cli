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
use std::path::PathBuf;

/// Handle the `+send` subcommand.
pub(super) async fn handle_send(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_send_args(matches)?;

    let raw = create_send_raw_message(&config)?;

    super::send_raw_email(doc, matches, &raw, None, None).await
}

pub(super) struct SendConfig {
    pub to: Vec<Mailbox>,
    pub subject: String,
    pub body: String,
    pub from: Option<Vec<Mailbox>>,
    pub cc: Option<Vec<Mailbox>>,
    pub bcc: Option<Vec<Mailbox>>,
    pub html: bool,
    pub attachments: Vec<PathBuf>,
}

fn create_send_raw_message(config: &SendConfig) -> Result<String, GwsError> {
    let mb = mail_builder::MessageBuilder::new()
        .to(to_mb_address_list(&config.to))
        .subject(&config.subject);

    let mb = apply_optional_headers(
        mb,
        config.from.as_deref(),
        config.cc.as_deref(),
        config.bcc.as_deref(),
    );

    if config.attachments.is_empty() {
        finalize_message(mb, &config.body, config.html)
    } else {
        finalize_message_with_attachments(mb, &config.body, config.html, &config.attachments)
    }
}

/// Build a multipart/mixed message with file attachments using mail-builder.
fn finalize_message_with_attachments(
    mb: mail_builder::MessageBuilder<'_>,
    body: &str,
    html: bool,
    attachments: &[PathBuf],
) -> Result<String, GwsError> {
    let mut mb = if html {
        mb.html_body(body.to_string())
    } else {
        mb.text_body(body.to_string())
    };

    for path in attachments {
        let data = std::fs::read(path).map_err(|e| {
            GwsError::Other(anyhow::anyhow!(
                "Failed to read attachment '{}': {e}",
                path.display()
            ))
        })?;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();

        let content_type = mime_type_from_path(path);

        mb = mb.attachment(content_type, filename, data);
    }

    mb.write_to_string()
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to serialize email: {e}")))
}

/// Detect MIME type from file extension, falling back to application/octet-stream.
fn mime_type_from_path(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "gz" | "gzip" => "application/gzip",
        "tar" => "application/x-tar",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn parse_send_args(matches: &ArgMatches) -> Result<SendConfig, GwsError> {
    let to = Mailbox::parse_list(matches.get_one::<String>("to").unwrap());
    if to.is_empty() {
        return Err(GwsError::Validation(
            "--to must specify at least one recipient".to_string(),
        ));
    }

    let raw_attachments: Vec<PathBuf> = matches
        .get_many::<String>("attachment")
        .map(|vals| vals.map(PathBuf::from).collect())
        .unwrap_or_default();

    // Validate and canonicalize attachment paths to prevent TOCTOU races.
    let mut attachments = Vec::with_capacity(raw_attachments.len());
    for path in &raw_attachments {
        let path_str = path.to_string_lossy();
        let canonical = crate::validate::validate_safe_file_path(&path_str, "--attachment")?;
        if !canonical.exists() {
            return Err(GwsError::Validation(format!(
                "Attachment file not found: {}",
                path.display()
            )));
        }
        if !canonical.is_file() {
            return Err(GwsError::Validation(format!(
                "Attachment path is not a file: {}",
                path.display()
            )));
        }
        attachments.push(canonical);
    }

    Ok(SendConfig {
        to,
        subject: matches.get_one::<String>("subject").unwrap().to_string(),
        body: matches.get_one::<String>("body").unwrap().to_string(),
        from: parse_optional_mailboxes(matches, "from"),
        cc: parse_optional_mailboxes(matches, "cc"),
        bcc: parse_optional_mailboxes(matches, "bcc"),
        html: matches.get_flag("html"),
        attachments,
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{extract_header, strip_qp_soft_breaks};
    use super::*;

    fn make_matches_send(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("subject").long("subject"))
            .arg(Arg::new("body").long("body"))
            .arg(Arg::new("from").long("from"))
            .arg(Arg::new("cc").long("cc"))
            .arg(Arg::new("bcc").long("bcc"))
            .arg(Arg::new("html").long("html").action(ArgAction::SetTrue))
            .arg(
                Arg::new("attachment")
                    .long("attachment")
                    .action(ArgAction::Append),
            );
        cmd.try_get_matches_from(args).unwrap()
    }

    fn default_config() -> SendConfig {
        SendConfig {
            to: Mailbox::parse_list("bob@example.com"),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            from: None,
            cc: None,
            bcc: None,
            html: false,
            attachments: vec![],
        }
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
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.to.len(), 1);
        assert_eq!(config.to[0].email, "me@example.com");
        assert_eq!(config.subject, "Hi");
        assert_eq!(config.body, "Body");
        assert!(config.from.is_none());
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
        assert!(config.attachments.is_empty());
    }

    #[test]
    fn test_parse_send_args_with_from() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--from",
            "alias@example.com",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.from.as_ref().unwrap()[0].email, "alias@example.com");

        // Whitespace-only --from becomes None
        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--from",
            "  ",
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert!(config.from.is_none());
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
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.cc.as_ref().unwrap()[0].email, "carol@example.com");
        assert_eq!(config.bcc.as_ref().unwrap()[0].email, "secret@example.com");

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
        let config = parse_send_args(&matches).unwrap();
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
        let config = parse_send_args(&matches).unwrap();
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
        let config = parse_send_args(&matches).unwrap();
        assert!(!config.html);
    }

    #[test]
    fn test_parse_send_args_empty_to_returns_error() {
        let matches = make_matches_send(&["test", "--to", "", "--subject", "Hi", "--body", "Body"]);
        let err = parse_send_args(&matches).err().unwrap();
        assert!(
            err.to_string().contains("--to"),
            "error should mention --to"
        );
    }

    #[test]
    fn test_send_html_raw_message() {
        let config = SendConfig {
            body: "<p>Hello <b>world</b></p>".to_string(),
            subject: "HTML test".to_string(),
            to: Mailbox::parse_list("bob@example.com"),
            html: true,
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();
        let decoded = strip_qp_soft_breaks(&raw);

        assert!(decoded.contains("text/html"));
        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("bob@example.com"));
        assert!(extract_header(&raw, "Subject")
            .unwrap()
            .contains("HTML test"));
        assert!(decoded.contains("<p>Hello <b>world</b></p>"));
        assert!(extract_header(&raw, "Cc").is_none());
    }

    #[test]
    fn test_send_plain_text_raw_message() {
        let config = SendConfig {
            subject: "Hello".to_string(),
            body: "World".to_string(),
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("bob@example.com"));
        assert!(extract_header(&raw, "Subject").unwrap().contains("Hello"));
        assert!(raw.contains("text/plain"));
        assert!(raw.contains("World"));
    }

    #[test]
    fn test_send_with_cc_and_bcc() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            cc: Some(Mailbox::parse_list("carol@example.com")),
            bcc: Some(Mailbox::parse_list("secret@example.com")),
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("alice@example.com"));
        assert!(extract_header(&raw, "Cc")
            .unwrap()
            .contains("carol@example.com"));
        assert!(extract_header(&raw, "Bcc")
            .unwrap()
            .contains("secret@example.com"));
        // Verify no leakage between headers
        assert!(!extract_header(&raw, "To")
            .unwrap()
            .contains("carol@example.com"));
        assert!(!extract_header(&raw, "To")
            .unwrap()
            .contains("secret@example.com"));
    }

    #[test]
    fn test_send_with_from() {
        let config = SendConfig {
            from: Some(Mailbox::parse_list("alias@example.com")),
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(extract_header(&raw, "From")
            .unwrap()
            .contains("alias@example.com"));
        assert!(extract_header(&raw, "To")
            .unwrap()
            .contains("bob@example.com"));
    }

    #[test]
    fn test_send_without_from_has_no_from_header() {
        let config = default_config();
        let raw = create_send_raw_message(&config).unwrap();
        assert!(extract_header(&raw, "From").is_none());
    }

    #[test]
    fn test_send_multiple_to_recipients() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com, bob@example.com"),
            subject: "Group".to_string(),
            body: "Hi all".to_string(),
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();
        let to_header = extract_header(&raw, "To").unwrap();
        assert!(to_header.contains("alice@example.com"));
        assert!(to_header.contains("bob@example.com"));
    }

    #[test]
    fn test_send_crlf_injection_in_from_does_not_create_header() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            from: Some(Mailbox::parse_list(
                "sender@example.com\r\nBcc: evil@attacker.com",
            )),
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();

        // The CRLF injection should not create a Bcc header
        assert!(
            extract_header(&raw, "Bcc").is_none(),
            "CRLF injection via --from should not create Bcc header"
        );
        // The From header should contain the sanitized email
        assert!(extract_header(&raw, "From")
            .unwrap()
            .contains("sender@example.com"));
    }

    #[test]
    fn test_send_crlf_injection_in_cc_does_not_create_header() {
        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            cc: Some(Mailbox::parse_list("carol@example.com\r\nX-Injected: yes")),
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();

        // CRLF stripped → "X-Injected: yes" is concatenated into the email,
        // not emitted as a separate header line
        assert!(
            extract_header(&raw, "X-Injected").is_none(),
            "CRLF injection via --cc should not create X-Injected header"
        );
        assert!(extract_header(&raw, "Cc")
            .unwrap()
            .contains("carol@example.com"));
    }

    // --- MIME type detection tests ---

    #[test]
    fn test_mime_type_from_path_common_types() {
        assert_eq!(
            mime_type_from_path(std::path::Path::new("report.pdf")),
            "application/pdf"
        );
        assert_eq!(
            mime_type_from_path(std::path::Path::new("doc.docx")),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(
            mime_type_from_path(std::path::Path::new("image.png")),
            "image/png"
        );
        assert_eq!(
            mime_type_from_path(std::path::Path::new("photo.jpg")),
            "image/jpeg"
        );
        assert_eq!(
            mime_type_from_path(std::path::Path::new("data.csv")),
            "text/csv"
        );
        assert_eq!(
            mime_type_from_path(std::path::Path::new("archive.zip")),
            "application/zip"
        );
    }

    #[test]
    fn test_mime_type_from_path_unknown_fallback() {
        assert_eq!(
            mime_type_from_path(std::path::Path::new("file.xyz")),
            "application/octet-stream"
        );
        assert_eq!(
            mime_type_from_path(std::path::Path::new("noextension")),
            "application/octet-stream"
        );
    }

    // --- Attachment tests ---

    #[test]
    fn test_send_with_attachment() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello attachment").unwrap();

        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "With attachment".to_string(),
            body: "See attached.".to_string(),
            attachments: vec![file_path],
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();

        // Should be multipart/mixed
        assert!(
            raw.contains("multipart/mixed"),
            "should be multipart: {raw}"
        );
        // Should contain the attachment filename
        assert!(raw.contains("test.txt"), "should reference filename: {raw}");
        // Should still contain the body text
        assert!(raw.contains("See attached."), "should contain body: {raw}");
    }

    #[test]
    fn test_send_with_multiple_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("doc.pdf");
        let file2 = dir.path().join("image.png");
        std::fs::write(&file1, b"fake pdf").unwrap();
        std::fs::write(&file2, b"fake png").unwrap();

        let config = SendConfig {
            to: Mailbox::parse_list("alice@example.com"),
            subject: "Multi-attach".to_string(),
            body: "Files attached.".to_string(),
            attachments: vec![file1, file2],
            ..default_config()
        };
        let raw = create_send_raw_message(&config).unwrap();

        assert!(raw.contains("doc.pdf"), "should reference doc.pdf: {raw}");
        assert!(
            raw.contains("image.png"),
            "should reference image.png: {raw}"
        );
    }

    #[test]
    fn test_send_attachment_nonexistent_file_fails() {
        let config = SendConfig {
            attachments: vec![PathBuf::from("/tmp/nonexistent_file_12345.pdf")],
            ..default_config()
        };
        let err = create_send_raw_message(&config).unwrap_err();
        assert!(
            err.to_string().contains("Failed to read attachment"),
            "error should mention failed read: {err}"
        );
    }

    #[test]
    fn test_attachment_path_traversal_rejected() {
        let matches = make_matches_send(&[
            "test",
            "--to",
            "bob@example.com",
            "--subject",
            "Hi",
            "--body",
            "Body",
            "--attachment",
            "../../etc/passwd",
        ]);
        let err = parse_send_args(&matches).err().unwrap();
        assert!(
            err.to_string().contains("path traversal")
                || err.to_string().contains("unsafe")
                || err.to_string().contains(".."),
            "should reject path traversal: {err}"
        );
    }
}

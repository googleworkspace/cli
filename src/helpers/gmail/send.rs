use super::*;
use std::path::PathBuf;

pub(super) async fn handle_send(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_send_args(matches)?;

    let builder = MessageBuilder {
        to: &config.to,
        subject: &config.subject,
        from: None,
        cc: config.cc.as_deref(),
        bcc: config.bcc.as_deref(),
        threading: None,
        html: config.html,
    };

    let raw = if config.attachments.is_empty() {
        builder.build(&config.body)
    } else {
        let attachments = load_attachments(&config.attachments)?;
        builder.build_with_attachments(&config.body, &attachments)
    };

    super::send_raw_email(doc, matches, &raw, None, None).await
}

pub(super) struct SendConfig {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub html: bool,
    pub attachments: Vec<PathBuf>,
}

fn parse_send_args(matches: &ArgMatches) -> Result<SendConfig, GwsError> {
    let attachments: Vec<PathBuf> = matches
        .get_many::<String>("attachment")
        .unwrap_or_default()
        .map(|s| PathBuf::from(s.trim()))
        .collect();

    // Validate each attachment path
    for path in &attachments {
        validate_attachment_path(path)?;
    }

    Ok(SendConfig {
        to: matches.get_one::<String>("to").unwrap().to_string(),
        subject: matches.get_one::<String>("subject").unwrap().to_string(),
        body: matches.get_one::<String>("body").unwrap().to_string(),
        cc: parse_optional_trimmed(matches, "cc"),
        bcc: parse_optional_trimmed(matches, "bcc"),
        html: matches.get_flag("html"),
        attachments,
    })
}

/// Validate that an attachment path is safe to read.
///
/// Rejects absolute paths, paths with control characters, and paths that
/// resolve (after following symlinks) to a location outside the current
/// working directory. The file must exist and be a regular file.
fn validate_attachment_path(path: &PathBuf) -> Result<(), GwsError> {
    let path_str = path.to_string_lossy();

    // Reject control characters
    if path_str.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return Err(GwsError::Validation(format!(
            "--attachment path contains invalid control characters: {}",
            path_str
        )));
    }

    // Reject absolute paths — force CWD-relative access
    if path.is_absolute() {
        return Err(GwsError::Validation(format!(
            "--attachment path must be relative: {}",
            path_str
        )));
    }

    // Resolve symlinks and normalize the path (also checks existence)
    let canonical_path = path.canonicalize().map_err(|e| {
        GwsError::Validation(format!(
            "Failed to resolve attachment path '{}': {}. Ensure the file exists and is accessible.",
            path_str, e
        ))
    })?;

    // Ensure the resolved path is within the current working directory
    let cwd = std::env::current_dir().map_err(|e| {
        GwsError::Validation(format!("Failed to determine current directory: {e}"))
    })?;
    let canonical_cwd = cwd.canonicalize().map_err(|e| {
        GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
    })?;

    if !canonical_path.starts_with(&canonical_cwd) {
        return Err(GwsError::Validation(format!(
            "--attachment path '{}' resolves outside the current directory",
            path_str
        )));
    }

    // Must be a regular file (not a directory or device)
    if !canonical_path.is_file() {
        return Err(GwsError::Validation(format!(
            "--attachment path is not a file: {}",
            path_str
        )));
    }

    Ok(())
}

/// Read attachment files from disk and prepare them for MIME encoding.
fn load_attachments(paths: &[PathBuf]) -> Result<Vec<super::Attachment>, GwsError> {
    let mut attachments = Vec::with_capacity(paths.len());

    for path in paths {
        let data = std::fs::read(path).map_err(|e| {
            GwsError::Validation(format!(
                "Failed to read attachment '{}': {}",
                path.display(),
                e
            ))
        })?;

        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "attachment".to_string());

        let mime_type = super::mime_type_from_extension(&filename).to_string();

        attachments.push(super::Attachment {
            filename,
            mime_type,
            data,
        });
    }

    Ok(attachments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn make_matches_send(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("to").long("to"))
            .arg(Arg::new("subject").long("subject"))
            .arg(Arg::new("body").long("body"))
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
        assert_eq!(config.to, "me@example.com");
        assert_eq!(config.subject, "Hi");
        assert_eq!(config.body, "Body");
        assert!(config.cc.is_none());
        assert!(config.bcc.is_none());
        assert!(config.attachments.is_empty());
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

    #[test]
    fn test_parse_send_args_with_attachments() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("doc.pdf");
        let file2 = dir.path().join("image.png");
        fs::write(&file1, b"fake pdf").unwrap();
        fs::write(&file2, b"fake png").unwrap();

        let f1 = file1.to_string_lossy().to_string();
        let f2 = file2.to_string_lossy().to_string();

        let matches = make_matches_send(&[
            "test",
            "--to",
            "me@example.com",
            "--subject",
            "Hi",
            "--body",
            "See attached",
            "--attachment",
            &f1,
            "--attachment",
            &f2,
        ]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.attachments.len(), 2);
    }

    #[test]
    fn test_validate_attachment_path_rejects_missing_file() {
        let path = PathBuf::from("nonexistent_file_12345.pdf");
        let result = validate_attachment_path(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Failed to resolve") || msg.contains("not found"),
            "got: {msg}"
        );
    }

    #[test]
    fn test_validate_attachment_path_rejects_absolute() {
        let path = if cfg!(windows) {
            PathBuf::from("C:\\Windows\\System32\\notepad.exe")
        } else {
            PathBuf::from("/etc/passwd")
        };
        let result = validate_attachment_path(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("must be relative"), "got: {msg}");
    }

    #[test]
    #[serial_test::serial]
    fn test_validate_attachment_path_rejects_directory() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let sub = canonical_dir.join("subdir");
        fs::create_dir(&sub).unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        let result = validate_attachment_path(&PathBuf::from("subdir"));
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not a file"), "got: {msg}");
    }

    #[test]
    fn test_validate_attachment_path_rejects_control_chars() {
        let path = PathBuf::from("file\x01name.pdf");
        let result = validate_attachment_path(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("control characters"), "got: {msg}");
    }

    #[test]
    #[serial_test::serial]
    fn test_validate_attachment_path_accepts_valid_file() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let file = canonical_dir.join("valid.pdf");
        fs::write(&file, b"data").unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        let result = validate_attachment_path(&PathBuf::from("valid.pdf"));
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[test]
    fn test_load_attachments() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("report.pdf");
        fs::write(&file, b"PDF content here").unwrap();

        let attachments = load_attachments(&[file]).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "report.pdf");
        assert_eq!(attachments[0].mime_type, "application/pdf");
        assert_eq!(attachments[0].data, b"PDF content here");
    }

    #[test]
    fn test_build_with_attachments() {
        let attachment = super::super::Attachment {
            filename: "test.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            data: b"fake pdf data".to_vec(),
        };

        let raw = MessageBuilder {
            to: "bob@example.com",
            subject: "With attachment",
            from: None,
            cc: None,
            bcc: None,
            threading: None,
            html: false,
        }
        .build_with_attachments("See attached.", &[attachment]);

        assert!(raw.contains("Content-Type: multipart/mixed; boundary="));
        assert!(raw.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(raw.contains("See attached."));
        assert!(raw.contains("Content-Disposition: attachment; filename=\"test.pdf\""));
        assert!(raw.contains("Content-Type: application/pdf"));
        assert!(raw.contains("Content-Transfer-Encoding: base64"));
    }

    #[test]
    fn test_build_with_attachments_escapes_filename_quotes() {
        let attachment = super::super::Attachment {
            filename: "my\"file.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            data: b"data".to_vec(),
        };

        let raw = MessageBuilder {
            to: "bob@example.com",
            subject: "Test",
            from: None,
            cc: None,
            bcc: None,
            threading: None,
            html: false,
        }
        .build_with_attachments("Body.", &[attachment]);

        assert!(
            raw.contains("filename=\"my\\\"file.pdf\""),
            "quotes in filename should be escaped: {raw}"
        );
    }

    #[test]
    fn test_build_with_empty_attachments_falls_back() {
        let raw = MessageBuilder {
            to: "bob@example.com",
            subject: "No attachments",
            from: None,
            cc: None,
            bcc: None,
            threading: None,
            html: false,
        }
        .build_with_attachments("Plain body.", &[]);

        // Should fall back to simple message (no multipart)
        assert!(raw.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(!raw.contains("multipart"));
    }
}

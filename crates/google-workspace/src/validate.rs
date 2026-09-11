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

//! Shared input validation helpers.
//!
//! These functions harden inputs against adversarial or accidentally
//! malformed values — especially important when the CLI is invoked by an
//! LLM agent rather than a human operator.

use crate::error::GwsError;
use std::path::{Path, PathBuf};

// ── Dangerous character detection ─────────────────────────────────────

/// Returns `true` for Unicode characters that are dangerous in terminal
/// output but not caught by `char::is_control()`: zero-width chars, bidi
/// overrides, Unicode line/paragraph separators, and directional isolates.
pub fn is_dangerous_unicode(c: char) -> bool {
    matches!(c,
        // zero-width: ZWSP, ZWNJ, ZWJ, BOM/ZWNBSP
        '\u{200B}'..='\u{200D}' | '\u{FEFF}' |
        // bidi: LRE, RLE, PDF, LRO, RLO
        '\u{202A}'..='\u{202E}' |
        // line / paragraph separators
        '\u{2028}'..='\u{2029}' |
        // directional isolates: LRI, RLI, FSI, PDI
        '\u{2066}'..='\u{2069}'
    )
}

/// Rejects strings containing control characters (C0: U+0000–U+001F,
/// C1: U+0080–U+009F, and DEL: U+007F) or dangerous Unicode characters
/// such as zero-width chars, bidi overrides, and line/paragraph separators.
///
/// Used for validating argument values at the parse boundary.
pub fn reject_dangerous_chars(value: &str, flag_name: &str) -> Result<(), GwsError> {
    for c in value.chars() {
        if c.is_control() {
            return Err(GwsError::Validation(format!(
                "{flag_name} contains invalid control characters"
            )));
        }
        if is_dangerous_unicode(c) {
            return Err(GwsError::Validation(format!(
                "{flag_name} contains invalid Unicode characters"
            )));
        }
    }
    Ok(())
}

// ── Path validators ───────────────────────────────────────────────────

/// Validates that `dir` is a safe output directory.
///
/// The path is resolved relative to CWD. The function rejects paths that
/// would escape above CWD (e.g. `../../.ssh`) or contain null bytes /
/// control characters.
///
/// Returns the canonicalized path on success.
pub fn validate_safe_output_dir(dir: &str) -> Result<PathBuf, GwsError> {
    reject_dangerous_chars(dir, "--output-dir")?;

    let path = Path::new(dir);

    // Reject absolute paths — force everything relative to CWD
    if path.is_absolute() {
        return Err(GwsError::Validation(format!(
            "--output-dir must be a relative path, got absolute path '{}'",
            dir
        )));
    }

    // Canonicalize CWD and resolve the target under it
    let cwd = std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))?;
    let resolved = cwd.join(path);

    // If the directory already exists, canonicalize. Otherwise, canonicalize
    // the longest existing prefix and append the remaining segments.
    let canonical = if resolved.exists() {
        resolved.canonicalize().map_err(|e| {
            GwsError::Validation(format!("Failed to resolve --output-dir '{}': {e}", dir))
        })?
    } else {
        normalize_non_existing(&resolved)?
    };

    let canonical_cwd = cwd.canonicalize().map_err(|e| {
        GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
    })?;

    if !canonical.starts_with(&canonical_cwd) {
        return Err(GwsError::Validation(format!(
            "--output-dir '{}' resolves to '{}' which is outside the current directory",
            dir,
            canonical.display()
        )));
    }

    Ok(canonical)
}

/// Validates that `dir` is a safe directory for reading files (e.g. `--dir`
/// in `script +push`).
///
/// Similar to [`validate_safe_output_dir`] but also follows symlinks
/// safely and ensures the resolved path stays under CWD.
pub fn validate_safe_dir_path(dir: &str) -> Result<PathBuf, GwsError> {
    reject_dangerous_chars(dir, "--dir")?;

    let path = Path::new(dir);

    // "." is always safe (CWD itself)
    if dir == "." {
        return std::env::current_dir().map_err(|e| {
            GwsError::Validation(format!("Failed to determine current directory: {e}"))
        });
    }

    if path.is_absolute() {
        return Err(GwsError::Validation(format!(
            "--dir must be a relative path, got absolute path '{}'",
            dir
        )));
    }

    let cwd = std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))?;
    let resolved = cwd.join(path);

    let canonical = resolved
        .canonicalize()
        .map_err(|e| GwsError::Validation(format!("Failed to resolve --dir '{}': {e}", dir)))?;

    let canonical_cwd = cwd.canonicalize().map_err(|e| {
        GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
    })?;

    if !canonical.starts_with(&canonical_cwd) {
        return Err(GwsError::Validation(format!(
            "--dir '{}' resolves to '{}' which is outside the current directory",
            dir,
            canonical.display()
        )));
    }

    Ok(canonical)
}

/// Validates that a file path (e.g. `--upload` or `--output`) is safe.
///
/// By default, the resolved target must live under CWD. The trusted operator
/// environment variable `GOOGLE_WORKSPACE_CLI_FILE_ROOT` can select a different
/// boundary: an existing directory, canonicalized before validation. With an
/// explicit root, CLI paths must not contain `..` components. Relative CLI paths
/// always resolve from CWD, not from the configured root. Absolute paths within
/// the boundary are allowed. Control characters and symlink escapes are rejected.
/// Directory validators do not use this setting.
///
/// # TOCTOU caveat
///
/// This is a best-effort defence-in-depth check. A local attacker with
/// write access to a parent directory could replace a path component
/// between this validation and the subsequent I/O. Fully eliminating
/// TOCTOU would require `openat(O_NOFOLLOW)` on each path component,
/// which is tracked as a follow-up for Unix platforms.
pub fn validate_safe_file_path(path_str: &str, flag_name: &str) -> Result<PathBuf, GwsError> {
    let cwd = std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))?;
    let file_root = std::env::var_os("GOOGLE_WORKSPACE_CLI_FILE_ROOT");
    validate_file_path_with_root(
        path_str,
        flag_name,
        &cwd,
        file_root.as_deref().map(Path::new),
    )
}

/// Explicit policy keeps filesystem validation independent of process-global env.
fn validate_file_path_with_root(
    path_str: &str,
    flag_name: &str,
    cwd: &Path,
    file_root: Option<&Path>,
) -> Result<PathBuf, GwsError> {
    reject_dangerous_chars(path_str, flag_name)?;

    let canonical_root = if let Some(root) = file_root {
        if root.as_os_str().is_empty() {
            return Err(GwsError::Validation(
                "GOOGLE_WORKSPACE_CLI_FILE_ROOT must name an existing directory; got an empty value"
                    .to_string(),
            ));
        }
        // Environment is trusted: relative roots (including `..`) are valid.
        let canonical = cwd.join(root).canonicalize().map_err(|e| {
            GwsError::Validation(format!(
                "GOOGLE_WORKSPACE_CLI_FILE_ROOT {root:?} must name an existing directory: {e}"
            ))
        })?;
        if !canonical.is_dir() {
            return Err(GwsError::Validation(format!(
                "GOOGLE_WORKSPACE_CLI_FILE_ROOT {root:?} must name an existing directory"
            )));
        }
        canonical
    } else {
        cwd.canonicalize().map_err(|e| {
            GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
        })?
    };
    let boundary = if file_root.is_some() {
        format!("GOOGLE_WORKSPACE_CLI_FILE_ROOT directory {canonical_root:?}")
    } else {
        format!("current directory {canonical_root:?}")
    };

    let path = Path::new(path_str);
    if file_root.is_some()
        && path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(GwsError::Validation(format!(
            "{flag_name} must not contain parent traversal ('..') components within the {boundary}; use a path without '..'"
        )));
    }

    // Path::join preserves absolute arguments; relative arguments stay CWD-relative.
    let resolved = cwd.join(path);
    let canonical = canonicalize_file_path(&resolved).map_err(|e| {
        GwsError::Validation(format!(
            "Failed to resolve {flag_name} {path_str:?} within the {boundary}: {e}"
        ))
    })?;
    // Preserve default handling of paths that normalize safely within CWD.
    let canonical = normalize_dotdot(&canonical);

    if !canonical.starts_with(&canonical_root) {
        return Err(GwsError::Validation(format!(
            "{flag_name} {path_str:?} resolves to {canonical:?} which is outside the {boundary}; set GOOGLE_WORKSPACE_CLI_FILE_ROOT to an existing directory containing the intended file"
        )));
    }

    Ok(canonical)
}

/// Canonicalize the existing file or nearest existing parent, then append the
/// missing suffix. Unlike `exists()`, symlink_metadata does not mistake dangling
/// symlinks for missing files. Keep this stricter resolver local to file flags.
fn canonicalize_file_path(path: &Path) -> std::io::Result<PathBuf> {
    let mut current = path;
    let mut remaining = Vec::new();
    loop {
        match std::fs::symlink_metadata(current) {
            Ok(_) => {
                let mut canonical = current.canonicalize()?;
                if !remaining.is_empty() && !canonical.is_dir() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "existing file parent must be a directory",
                    ));
                }
                for component in remaining.into_iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = current.file_name().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "cannot resolve an existing directory prefix",
                    )
                })?;
                remaining.push(name);
                current = current.parent().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "cannot resolve a file parent",
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Resolve `.` and `..` components in a path without touching the filesystem.
fn normalize_dotdot(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Resolves a path that may not exist yet by canonicalizing the existing
/// prefix and appending remaining components.
fn normalize_non_existing(path: &Path) -> Result<PathBuf, GwsError> {
    let mut resolved = PathBuf::new();
    let mut remaining = Vec::new();

    // Walk backwards until we find a component that exists
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            resolved = current
                .canonicalize()
                .map_err(|e| GwsError::Validation(format!("Failed to canonicalize path: {e}")))?;
            break;
        }
        if let Some(name) = current.file_name() {
            remaining.push(name.to_os_string());
        } else {
            // We've exhausted the path without finding an existing prefix
            return Err(GwsError::Validation(format!(
                "Cannot resolve path '{}'",
                path.display()
            )));
        }
        current = match current.parent() {
            Some(p) => p.to_path_buf(),
            None => break,
        };
    }

    // Append remaining segments (in reverse since we collected them backwards)
    for seg in remaining.into_iter().rev() {
        resolved.push(seg);
    }

    Ok(resolved)
}

// ── URL encoding ──────────────────────────────────────────────────────

/// Percent-encode a value for use as a single URL path segment (e.g., file ID,
/// calendar ID, message ID). All non-alphanumeric characters are encoded.
pub fn encode_path_segment(s: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

/// Percent-encode a value for use in URI path templates where `/` should stay
/// as a path separator (e.g., RFC 6570 `{+name}` expansions).
///
/// Each path segment is encoded independently, then joined with `/`, so
/// dangerous characters like `#`/`?` are still escaped while hierarchical
/// resource names such as `projects/p/locations/l` remain readable.
pub fn encode_path_preserving_slashes(s: &str) -> String {
    s.split('/')
        .map(encode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

// ── Resource / API validators ─────────────────────────────────────────

/// Validate a multi-segment resource name (e.g., `spaces/ABC`, `subscriptions/123`).
/// Rejects path traversal, control characters, and URL-special characters including `%`
/// to prevent URL-encoded bypasses. Returns the validated name or an error.
pub fn validate_resource_name(s: &str) -> Result<&str, GwsError> {
    if s.is_empty() {
        return Err(GwsError::Validation(
            "Resource name must not be empty".to_string(),
        ));
    }
    if s.split('/').any(|seg| seg == "..") {
        return Err(GwsError::Validation(format!(
            "Resource name must not contain path traversal ('..') segments: {s}"
        )));
    }
    if s.chars()
        .any(|c| c == '\0' || c.is_control() || is_dangerous_unicode(c))
    {
        return Err(GwsError::Validation(format!(
            "Resource name contains invalid characters: {s}"
        )));
    }
    // Reject URL-special characters that could inject query params or fragments
    if s.contains('?') || s.contains('#') {
        return Err(GwsError::Validation(format!(
            "Resource name must not contain '?' or '#': {s}"
        )));
    }
    // Reject '%' to prevent URL-encoded bypasses (e.g. %2e%2e for ..)
    if s.contains('%') {
        return Err(GwsError::Validation(format!(
            "Resource name must not contain '%' (URL encoding bypass attempt): {s}"
        )));
    }
    Ok(s)
}

/// Validate an API identifier (service name, version string) for use in
/// cache filenames and discovery URLs. Only alphanumeric characters, hyphens,
/// underscores, and dots are allowed to prevent path traversal and injection.
pub fn validate_api_identifier(s: &str) -> Result<&str, GwsError> {
    if s.is_empty() {
        return Err(GwsError::Validation(
            "API identifier must not be empty".to_string(),
        ));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(GwsError::Validation(format!(
            "API identifier contains invalid characters (only alphanumeric, '-', '_', '.' allowed): {s}"
        )));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    // --- validate_safe_output_dir ---

    #[test]
    #[serial]
    fn test_output_dir_relative_subdir() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let sub = canonical_dir.join("output");
        fs::create_dir_all(&sub).unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        let result = validate_safe_output_dir("output");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[test]
    #[serial]
    fn test_output_dir_rejects_symlink_traversal() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        let allowed_dir = canonical_dir.join("allowed");
        fs::create_dir(&allowed_dir).unwrap();

        let symlink_path = canonical_dir.join("sneaky_link");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/tmp", &symlink_path).unwrap();
        #[cfg(windows)]
        return;

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        let result = validate_safe_output_dir("sneaky_link");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("outside the current directory"), "got: {msg}");
    }

    #[test]
    #[serial]
    fn test_output_dir_rejects_traversal() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        let result = validate_safe_output_dir("../../.ssh");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("outside the current directory"), "got: {msg}");
    }

    #[test]
    fn test_output_dir_rejects_absolute() {
        assert!(validate_safe_output_dir("/tmp/evil").is_err());
    }

    #[test]
    fn test_output_dir_rejects_null_bytes() {
        assert!(validate_safe_output_dir("foo\0bar").is_err());
    }

    #[test]
    fn test_output_dir_rejects_control_chars() {
        assert!(validate_safe_output_dir("foo\x01bar").is_err());
    }

    #[test]
    #[serial]
    fn test_output_dir_non_existing_subdir() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        let result = validate_safe_output_dir("new/nested/dir");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(
            result.is_ok(),
            "expected Ok for non-existing subdir, got: {result:?}"
        );
    }

    // --- validate_safe_dir_path ---

    #[test]
    fn test_dir_path_cwd() {
        assert!(validate_safe_dir_path(".").is_ok());
    }

    #[test]
    #[serial]
    fn test_dir_path_rejects_traversal() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        let result = validate_safe_dir_path("../../etc");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
    }

    #[test]
    fn test_dir_path_rejects_absolute() {
        assert!(validate_safe_dir_path("/usr/local").is_err());
    }

    // --- reject_dangerous_chars ---

    #[test]
    fn test_reject_dangerous_chars_clean() {
        assert!(reject_dangerous_chars("hello/world", "test").is_ok());
    }

    #[test]
    fn test_reject_dangerous_chars_tab() {
        assert!(reject_dangerous_chars("hello\tworld", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_newline() {
        assert!(reject_dangerous_chars("hello\nworld", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_del() {
        assert!(reject_dangerous_chars("hello\x7Fworld", "test").is_err());
    }

    // -- encode_path_segment --------------------------------------------------

    #[test]
    fn test_encode_path_segment_plain_id() {
        assert_eq!(encode_path_segment("abc123"), "abc123");
    }

    #[test]
    fn test_encode_path_segment_email() {
        let encoded = encode_path_segment("user@gmail.com");
        assert!(!encoded.contains('@'));
        assert!(!encoded.contains('.'));
    }

    #[test]
    fn test_encode_path_segment_query_injection() {
        let encoded = encode_path_segment("fileid?fields=name");
        assert!(!encoded.contains('?'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn test_encode_path_segment_fragment_injection() {
        let encoded = encode_path_segment("fileid#section");
        assert!(!encoded.contains('#'));
    }

    #[test]
    fn test_encode_path_segment_path_traversal() {
        let encoded = encode_path_segment("../../etc/passwd");
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains(".."));
    }

    #[test]
    fn test_encode_path_segment_unicode() {
        let encoded = encode_path_segment("日本語ID");
        assert!(!encoded.contains('日'));
    }

    #[test]
    fn test_encode_path_segment_spaces() {
        let encoded = encode_path_segment("my file id");
        assert!(!encoded.contains(' '));
    }

    #[test]
    fn test_encode_path_segment_already_encoded() {
        let encoded = encode_path_segment("user%40gmail.com");
        assert!(encoded.contains("%2540"));
    }

    #[test]
    fn test_encode_path_preserving_slashes_hierarchical_name() {
        let encoded = encode_path_preserving_slashes("projects/p1/locations/us/topics/t1");
        assert_eq!(encoded, "projects/p1/locations/us/topics/t1");
    }

    #[test]
    fn test_encode_path_preserving_slashes_escapes_reserved_chars() {
        let encoded = encode_path_preserving_slashes("hash#1/child?x=y");
        assert_eq!(encoded, "hash%231/child%3Fx%3Dy");
    }

    #[test]
    fn test_encode_path_preserving_slashes_spaces_and_unicode() {
        let encoded = encode_path_preserving_slashes("タイムライン 1/列 A");
        assert!(!encoded.contains(' '));
        assert!(encoded.contains('/'));
    }

    // -- validate_resource_name -----------------------------------------------

    #[test]
    fn test_validate_resource_name_valid() {
        assert!(validate_resource_name("spaces/ABC123").is_ok());
        assert!(validate_resource_name("subscriptions/my-sub").is_ok());
        assert!(validate_resource_name("@default").is_ok());
        assert!(validate_resource_name("projects/p1/topics/t1").is_ok());
    }

    #[test]
    fn test_validate_resource_name_traversal() {
        assert!(validate_resource_name("../../etc/passwd").is_err());
        assert!(validate_resource_name("spaces/../other").is_err());
        assert!(validate_resource_name("..").is_err());
    }

    #[test]
    fn test_validate_resource_name_control_chars() {
        assert!(validate_resource_name("spaces/\0bad").is_err());
        assert!(validate_resource_name("spaces/\nbad").is_err());
        assert!(validate_resource_name("spaces/\rbad").is_err());
        assert!(validate_resource_name("spaces/\tbad").is_err());
    }

    #[test]
    fn test_validate_resource_name_empty() {
        assert!(validate_resource_name("").is_err());
    }

    #[test]
    fn test_validate_resource_name_query_injection() {
        assert!(validate_resource_name("spaces/ABC?key=val").is_err());
        assert!(validate_resource_name("spaces/ABC#fragment").is_err());
    }

    #[test]
    fn test_validate_resource_name_error_messages_are_clear() {
        let err = validate_resource_name("").unwrap_err();
        assert!(err.to_string().contains("must not be empty"));

        let err = validate_resource_name("../bad").unwrap_err();
        assert!(err.to_string().contains("path traversal"));

        let err = validate_resource_name("bad\0id").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
    }

    #[test]
    fn test_validate_resource_name_percent_bypass() {
        assert!(validate_resource_name("%2e%2e").is_err());
        assert!(validate_resource_name("spaces/%2e%2e/etc").is_err());
        assert!(validate_resource_name("spaces/100%").is_err());
    }

    // --- reject_dangerous_chars Unicode ---

    #[test]
    fn test_reject_dangerous_chars_zero_width_space() {
        assert!(reject_dangerous_chars("foo\u{200B}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_bom() {
        assert!(reject_dangerous_chars("foo\u{FEFF}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_rtl_override() {
        assert!(reject_dangerous_chars("foo\u{202E}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_unicode_line_separator() {
        assert!(reject_dangerous_chars("foo\u{2028}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_paragraph_separator() {
        assert!(reject_dangerous_chars("foo\u{2029}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_zero_width_joiner() {
        assert!(reject_dangerous_chars("foo\u{200D}bar", "test").is_err());
    }

    #[test]
    fn test_reject_dangerous_chars_normal_unicode_ok() {
        assert!(reject_dangerous_chars("日本語", "test").is_ok());
        assert!(reject_dangerous_chars("café", "test").is_ok());
        assert!(reject_dangerous_chars("αβγ", "test").is_ok());
    }

    // --- path validator Unicode ---

    #[test]
    fn test_output_dir_rejects_zero_width_chars() {
        assert!(validate_safe_output_dir("foo\u{200B}bar").is_err());
    }

    #[test]
    fn test_output_dir_rejects_rtl_override() {
        assert!(validate_safe_output_dir("foo\u{202E}bar").is_err());
    }

    #[test]
    fn test_output_dir_rejects_unicode_line_separator() {
        assert!(validate_safe_output_dir("foo\u{2028}bar").is_err());
    }

    // --- validate_resource_name Unicode ---

    #[test]
    fn test_validate_resource_name_zero_width_chars() {
        assert!(validate_resource_name("foo\u{200B}bar").is_err());
        assert!(validate_resource_name("foo\u{200D}bar").is_err());
        assert!(validate_resource_name("foo\u{FEFF}bar").is_err());
    }

    #[test]
    fn test_validate_resource_name_unicode_line_seps() {
        assert!(validate_resource_name("foo\u{2028}bar").is_err());
        assert!(validate_resource_name("foo\u{2029}bar").is_err());
    }

    #[test]
    fn test_validate_resource_name_rtl_override() {
        assert!(validate_resource_name("foo\u{202E}bar").is_err());
    }

    #[test]
    fn test_validate_resource_name_bidi_embedding() {
        assert!(validate_resource_name("foo\u{202A}bar").is_err());
        assert!(validate_resource_name("foo\u{202B}bar").is_err());
    }

    #[test]
    fn test_validate_resource_name_homoglyphs_pass_through() {
        assert!(validate_resource_name("spaces/ΑΒС").is_ok());
    }

    #[test]
    fn test_validate_resource_name_overlong_accepted() {
        let long = "a".repeat(10_000);
        assert!(validate_resource_name(&long).is_ok());
    }

    // --- validate_api_identifier ---

    #[test]
    fn test_validate_api_identifier_valid() {
        assert_eq!(validate_api_identifier("drive").unwrap(), "drive");
        assert_eq!(validate_api_identifier("v3").unwrap(), "v3");
        assert_eq!(
            validate_api_identifier("directory_v1").unwrap(),
            "directory_v1"
        );
        assert_eq!(
            validate_api_identifier("admin.reports_v1").unwrap(),
            "admin.reports_v1"
        );
        assert_eq!(validate_api_identifier("v2beta1").unwrap(), "v2beta1");
    }

    #[test]
    fn test_validate_api_identifier_rejects_path_traversal() {
        assert!(validate_api_identifier("../etc/passwd").is_err());
        assert!(validate_api_identifier("foo/../bar").is_err());
    }

    #[test]
    fn test_validate_api_identifier_rejects_special_chars() {
        assert!(validate_api_identifier("drive?key=val").is_err());
        assert!(validate_api_identifier("drive#frag").is_err());
        assert!(validate_api_identifier("drive%2f..").is_err());
        assert!(validate_api_identifier("v3 ").is_err());
        assert!(validate_api_identifier("v3\n").is_err());
    }

    #[test]
    fn test_validate_api_identifier_empty() {
        assert!(validate_api_identifier("").is_err());
    }

    // --- validate_safe_file_path ---

    #[test]
    #[serial]
    fn test_file_path_relative_is_ok() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        fs::write(canonical_dir.join("test.txt"), "data").unwrap();

        let _environment = FilePathEnvironment::set(&canonical_dir, None);

        let result = validate_safe_file_path("test.txt", "--upload");

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[test]
    #[serial]
    fn test_file_path_rejects_traversal() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        let _environment = FilePathEnvironment::set(&canonical_dir, None);

        let result = validate_safe_file_path("../../etc/passwd", "--upload");

        assert!(result.is_err(), "path traversal should be rejected");
        assert!(
            result.unwrap_err().to_string().contains("outside"),
            "error should mention 'outside'"
        );
    }

    #[test]
    #[serial]
    fn test_file_path_rejects_control_chars() {
        let dir = tempdir().unwrap();
        let _environment = FilePathEnvironment::set(dir.path(), None);
        let result = validate_safe_file_path("file\x00.txt", "--output");
        assert!(result.is_err(), "null bytes should be rejected");
    }

    #[test]
    #[serial]
    fn test_file_path_rejects_symlink_escape() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        #[cfg(unix)]
        {
            let link_path = canonical_dir.join("escape");
            std::os::unix::fs::symlink("/tmp", &link_path).unwrap();

            let _environment = FilePathEnvironment::set(&canonical_dir, None);

            let result = validate_safe_file_path("escape/secret.txt", "--output");

            assert!(result.is_err(), "symlink escape should be rejected");
        }
    }

    #[test]
    #[serial]
    fn test_file_path_rejects_traversal_via_nonexistent_prefix() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        let _environment = FilePathEnvironment::set(&canonical_dir, None);

        let result = validate_safe_file_path("doesnt_exist/../../etc/passwd", "--output");

        assert!(
            result.is_err(),
            "traversal via non-existent prefix should be rejected"
        );
    }

    // Default public-validator and directory-scope tests isolate global state;
    // scoped file-policy tests pass CWD and the trusted root explicitly.
    struct FilePathEnvironment {
        cwd: PathBuf,
        root: Option<std::ffi::OsString>,
    }

    impl FilePathEnvironment {
        fn set(cwd: &Path, root: Option<&Path>) -> Self {
            let saved = Self {
                cwd: std::env::current_dir().unwrap(),
                root: std::env::var_os("GOOGLE_WORKSPACE_CLI_FILE_ROOT"),
            };
            std::env::set_current_dir(cwd).unwrap();
            match root {
                Some(root) => std::env::set_var("GOOGLE_WORKSPACE_CLI_FILE_ROOT", root),
                None => std::env::remove_var("GOOGLE_WORKSPACE_CLI_FILE_ROOT"),
            }
            saved
        }
    }

    impl Drop for FilePathEnvironment {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.cwd).unwrap();
            match &self.root {
                Some(root) => std::env::set_var("GOOGLE_WORKSPACE_CLI_FILE_ROOT", root),
                None => std::env::remove_var("GOOGLE_WORKSPACE_CLI_FILE_ROOT"),
            }
        }
    }

    fn file_path_under_root(
        path: &Path,
        flag: &str,
        cwd: &Path,
        root: Option<&Path>,
    ) -> Result<PathBuf, GwsError> {
        validate_file_path_with_root(path.to_str().unwrap(), flag, cwd, root)
    }

    #[test]
    fn file_root_default_preserves_cwd_boundary_and_resolution() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        fs::create_dir(cwd.join("nested")).unwrap();
        fs::write(cwd.join("upload.txt"), "synthetic upload").unwrap();
        for path in [cwd.join("upload.txt"), PathBuf::from("upload.txt")] {
            assert_eq!(
                file_path_under_root(&path, "--upload", &cwd, None).unwrap(),
                cwd.join("upload.txt")
            );
        }
        assert_eq!(
            file_path_under_root(Path::new("nested/../new.txt"), "--output", &cwd, None).unwrap(),
            cwd.join("new.txt")
        );
        for path in [
            cwd.parent().unwrap().join("outside.txt"),
            PathBuf::from("../outside.txt"),
        ] {
            let err = file_path_under_root(&path, "--output", &cwd, None)
                .unwrap_err()
                .to_string();
            assert!(err.contains("outside the current directory"), "{err}");
            assert!(err.contains("GOOGLE_WORKSPACE_CLI_FILE_ROOT"), "{err}");
        }
        assert!(file_path_under_root(
            Path::new("missing/../../outside.txt"),
            "--output",
            &cwd,
            None
        )
        .is_err());
    }

    #[test]
    fn file_root_accepts_absolute_output_and_existing_upload() {
        let cwd = tempdir().unwrap();
        let root = tempdir().unwrap();
        let canonical_root = root.path().canonicalize().unwrap();
        fs::write(root.path().join("upload.txt"), "synthetic upload").unwrap();
        for (name, flag) in [("new.txt", "--output"), ("upload.txt", "--upload")] {
            assert_eq!(
                file_path_under_root(&root.path().join(name), flag, cwd.path(), Some(root.path()))
                    .unwrap(),
                canonical_root.join(name)
            );
        }
        assert!(!root.path().join("new.txt").exists());
    }

    #[test]
    fn file_root_rejects_sibling_even_with_shared_name_prefix() {
        let dir = tempdir().unwrap();
        // A literal backslash on Unix, a separator on Windows; both must be
        // compared using the escaped canonical representation in diagnostics.
        let parent = dir.path().join(r"back\slash");
        fs::create_dir_all(&parent).unwrap();
        let root = parent.join("allowed");
        let sibling = parent.join("allowed-sibling");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&sibling).unwrap();
        let err = file_path_under_root(
            &sibling.join("new.txt"),
            "--output",
            dir.path(),
            Some(&root),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("outside"), "{err}");
        assert!(err.contains("GOOGLE_WORKSPACE_CLI_FILE_ROOT"), "{err}");
        assert!(
            err.contains(&format!("{:?}", root.canonicalize().unwrap())),
            "{err}"
        );
    }

    #[test]
    fn file_root_rejects_parent_components_even_inside_boundary() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("nested")).unwrap();
        for path in ["nested/../new.txt", "missing/../new.txt", "../outside.txt"] {
            assert!(
                file_path_under_root(Path::new(path), "--output", root.path(), Some(root.path()))
                    .is_err(),
                "accepted {path}"
            );
        }
    }

    #[test]
    fn file_root_rejects_control_and_dangerous_unicode_arguments() {
        let root = tempdir().unwrap();
        for path in [
            "bad\0.txt",
            "bad\n.txt",
            "bad\u{202e}.txt",
            "bad\u{200b}.txt",
        ] {
            assert!(
                file_path_under_root(Path::new(path), "--output", root.path(), Some(root.path()))
                    .is_err(),
                "accepted {path:?}"
            );
        }
    }

    #[test]
    fn file_root_rejects_invalid_roots_without_falling_back_to_cwd() {
        let cwd = tempdir().unwrap();
        let file = cwd.path().join("file.txt");
        fs::write(&file, "synthetic file").unwrap();
        for root in [PathBuf::new(), cwd.path().join("missing"), file] {
            let err =
                file_path_under_root(Path::new("new.txt"), "--output", cwd.path(), Some(&root))
                    .unwrap_err()
                    .to_string();
            assert!(err.contains("GOOGLE_WORKSPACE_CLI_FILE_ROOT"), "{err}");
            assert!(err.contains("directory"), "{err}");
        }
    }

    #[test]
    fn file_root_keeps_relative_arguments_cwd_relative() {
        let root = tempdir().unwrap();
        let cwd = root.path().join("working");
        fs::create_dir(&cwd).unwrap();
        assert_eq!(
            file_path_under_root(Path::new("new.txt"), "--output", &cwd, Some(root.path()))
                .unwrap(),
            cwd.canonicalize().unwrap().join("new.txt")
        );
        let unrelated = tempdir().unwrap();
        assert!(file_path_under_root(
            Path::new("new.txt"),
            "--output",
            unrelated.path(),
            Some(root.path())
        )
        .is_err());
    }

    #[test]
    fn file_root_canonicalizes_trusted_relative_root_with_parent_components() {
        let root = tempdir().unwrap();
        let cwd = root.path().join("working");
        fs::create_dir(&cwd).unwrap();
        assert_eq!(
            file_path_under_root(
                Path::new("new.txt"),
                "--output",
                &cwd,
                Some(Path::new(".."))
            )
            .unwrap(),
            cwd.canonicalize().unwrap().join("new.txt")
        );
    }

    #[test]
    fn file_root_validates_nested_new_file_parents_without_creating_them() {
        let cwd = tempdir().unwrap();
        let root = tempdir().unwrap();
        let output = root.path().join("new/nested/output.bin");
        assert_eq!(
            file_path_under_root(&output, "--output", cwd.path(), Some(root.path())).unwrap(),
            root.path()
                .canonicalize()
                .unwrap()
                .join("new/nested/output.bin")
        );
        assert!(!root.path().join("new").exists());
        let file = root.path().join("file.txt");
        fs::write(&file, "synthetic file").unwrap();
        assert!(file_path_under_root(
            &file.join("output.bin"),
            "--output",
            cwd.path(),
            Some(root.path())
        )
        .is_err());
    }

    #[test]
    #[serial]
    fn file_root_does_not_expand_directory_validators() {
        let cwd = tempdir().unwrap();
        let root = tempdir().unwrap();
        let _environment = FilePathEnvironment::set(cwd.path(), Some(root.path()));
        assert!(validate_safe_output_dir(root.path().to_str().unwrap()).is_err());
        assert!(validate_safe_dir_path(root.path().to_str().unwrap()).is_err());
        assert_eq!(
            validate_safe_output_dir("new").unwrap(),
            cwd.path().canonicalize().unwrap().join("new")
        );
        assert!(validate_safe_dir_path(".").is_ok());
    }

    #[test]
    #[serial]
    fn file_root_environment_is_restored_on_unwind() {
        let cwd = std::env::current_dir().unwrap();
        let root = std::env::var_os("GOOGLE_WORKSPACE_CLI_FILE_ROOT");
        let dir = tempdir().unwrap();
        let result = std::panic::catch_unwind(|| {
            let _environment = FilePathEnvironment::set(dir.path(), Some(dir.path()));
            panic!("exercise restoration");
        });
        assert!(result.is_err());
        assert_eq!(std::env::current_dir().unwrap(), cwd);
        assert_eq!(std::env::var_os("GOOGLE_WORKSPACE_CLI_FILE_ROOT"), root);
    }

    #[cfg(unix)]
    #[test]
    fn file_root_resolves_inside_symlinks_and_rejects_escapes() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::create_dir(root.path().join("inside")).unwrap();
        fs::write(root.path().join("inside/upload.txt"), "inside").unwrap();
        fs::write(outside.path().join("upload.txt"), "outside").unwrap();
        symlink(root.path().join("inside"), root.path().join("safe")).unwrap();
        symlink(outside.path(), root.path().join("escape")).unwrap();
        for (suffix, flag) in [("upload.txt", "--upload"), ("new/output.bin", "--output")] {
            assert_eq!(
                file_path_under_root(
                    &root.path().join("safe").join(suffix),
                    flag,
                    root.path(),
                    Some(root.path())
                )
                .unwrap(),
                root.path()
                    .canonicalize()
                    .unwrap()
                    .join("inside")
                    .join(suffix)
            );
            assert!(file_path_under_root(
                &root.path().join("escape").join(suffix),
                flag,
                root.path(),
                Some(root.path())
            )
            .is_err());
        }
        // A trusted root may itself be a symlink to an existing directory.
        assert_eq!(
            file_path_under_root(
                &root.path().join("safe/upload.txt"),
                "--upload",
                outside.path(),
                Some(&root.path().join("safe"))
            )
            .unwrap(),
            root.path()
                .canonicalize()
                .unwrap()
                .join("inside/upload.txt")
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_root_rejects_dangling_symlinks_and_loops() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path().join("new.txt"), root.path().join("dangling")).unwrap();
        symlink("loop", root.path().join("loop")).unwrap();
        for root_policy in [None, Some(root.path())] {
            for name in ["dangling", "dangling/new.txt", "loop", "loop/new.txt"] {
                assert!(
                    file_path_under_root(Path::new(name), "--output", root.path(), root_policy)
                        .is_err(),
                    "accepted {name}"
                );
            }
        }
    }
}

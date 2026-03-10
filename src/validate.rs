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
//! These functions harden CLI inputs against adversarial or accidentally
//! malformed values — especially important when the CLI is invoked by an
//! LLM agent rather than a human operator.

use crate::error::GwsError;
use std::path::{Path, PathBuf};

/// Validates that `dir` is a safe output directory.
///
/// The path is resolved relative to CWD. The function rejects paths that
/// would escape above CWD (e.g. `../../.ssh`) or contain null bytes /
/// control characters.
///
/// Returns the canonicalized path on success.
pub fn validate_safe_output_dir(dir: &str) -> Result<PathBuf, GwsError> {
    reject_control_chars(dir, "--output-dir")?;

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
    reject_control_chars(dir, "--dir")?;

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

/// Rejects strings containing null bytes or ASCII control characters
/// (including DEL, 0x7F).
fn reject_control_chars(value: &str, flag_name: &str) -> Result<(), GwsError> {
    if value.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return Err(GwsError::Validation(format!(
            "{flag_name} contains invalid control characters"
        )));
    }
    Ok(())
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
    if s.contains('\0') || s.chars().any(|c| c.is_control()) {
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

/// Validate a GCP region/location identifier (e.g. `us-central1`, `europe-west4`).
/// Only ASCII lowercase letters, digits, and hyphens are allowed — dots are
/// rejected to prevent SSRF when the location is interpolated into a hostname.
pub fn validate_gcp_location(s: &str) -> Result<&str, GwsError> {
    if s.is_empty() {
        return Err(GwsError::Validation(
            "GCP location must not be empty".to_string(),
        ));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(GwsError::Validation(format!(
            "GCP location contains invalid characters (only lowercase alphanumeric and '-' allowed): {s}"
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
        // Create a real temp dir and change into it for the test
        let dir = tempdir().unwrap();
        // Canonicalize to handle macOS /var -> /private/var symlink
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

        // Create a directory inside the tempdir
        let allowed_dir = canonical_dir.join("allowed");
        fs::create_dir(&allowed_dir).unwrap();

        // Create a symlink pointing OUTSIDE the tempdir (e.g. to /tmp)
        let symlink_path = canonical_dir.join("sneaky_link");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/tmp", &symlink_path).unwrap();
        #[cfg(windows)]
        return; // Skip on Windows due to privilege requirements for symlinks

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        // Try to validate the symlink resolving outside CWD
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

    // --- reject_control_chars ---

    #[test]
    fn test_reject_control_chars_clean() {
        assert!(reject_control_chars("hello/world", "test").is_ok());
    }

    #[test]
    fn test_reject_control_chars_tab() {
        assert!(reject_control_chars("hello\tworld", "test").is_err());
    }

    #[test]
    fn test_reject_control_chars_newline() {
        assert!(reject_control_chars("hello\nworld", "test").is_err());
    }

    #[test]
    fn test_reject_control_chars_del() {
        assert!(reject_control_chars("hello\x7Fworld", "test").is_err());
    }

    // -- encode_path_segment --------------------------------------------------

    #[test]
    fn test_encode_path_segment_plain_id() {
        assert_eq!(encode_path_segment("abc123"), "abc123");
    }

    #[test]
    fn test_encode_path_segment_email() {
        // Calendar IDs are often email addresses
        let encoded = encode_path_segment("user@gmail.com");
        assert!(!encoded.contains('@'));
        assert!(!encoded.contains('.'));
    }

    #[test]
    fn test_encode_path_segment_query_injection() {
        // LLM might include query params in an ID by mistake
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
        // Encoding makes traversal segments harmless
        let encoded = encode_path_segment("../../etc/passwd");
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains(".."));
    }

    #[test]
    fn test_encode_path_segment_unicode() {
        // LLM might pass unicode characters
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
        // LLM might double-encode by passing pre-encoded values
        let encoded = encode_path_segment("user%40gmail.com");
        // The % itself gets encoded to %25, so %40 becomes %2540
        // This prevents double-encoding issues at the HTTP layer
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
        // LLMs might append query strings or fragments to resource names
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
        // %2e%2e is ..
        assert!(validate_resource_name("%2e%2e").is_err());
        assert!(validate_resource_name("spaces/%2e%2e/etc").is_err());
        // Just % should be rejected too
        assert!(validate_resource_name("spaces/100%").is_err());
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

    // --- validate_gcp_location ---

    #[test]
    fn test_validate_gcp_location_valid() {
        assert_eq!(validate_gcp_location("us-central1").unwrap(), "us-central1");
        assert_eq!(
            validate_gcp_location("europe-west4").unwrap(),
            "europe-west4"
        );
        assert_eq!(validate_gcp_location("asia-east1").unwrap(), "asia-east1");
        assert_eq!(validate_gcp_location("global").unwrap(), "global");
    }

    #[test]
    fn test_validate_gcp_location_rejects_dots() {
        assert!(validate_gcp_location("evil.com").is_err());
        assert!(validate_gcp_location("internal.evil.com").is_err());
    }

    #[test]
    fn test_validate_gcp_location_rejects_uppercase() {
        assert!(validate_gcp_location("US-CENTRAL1").is_err());
    }

    #[test]
    fn test_validate_gcp_location_rejects_special_chars() {
        assert!(validate_gcp_location("us central1").is_err());
        assert!(validate_gcp_location("us/central1").is_err());
        assert!(validate_gcp_location("us_central1").is_err());
    }

    #[test]
    fn test_validate_gcp_location_empty() {
        assert!(validate_gcp_location("").is_err());
    }
}

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

/// Allowed values for Gmail message format (`--msg-format`).
pub const VALID_MSG_FORMATS: &[&str] = &["full", "metadata", "minimal", "raw"];

/// Validates that `value` is one of the allowed Gmail message formats.
///
/// Returns the validated value on success, or a descriptive
/// [`GwsError::Validation`] listing the allowed options.
pub fn validate_msg_format(value: &str) -> Result<&str, GwsError> {
    if VALID_MSG_FORMATS.contains(&value) {
        Ok(value)
    } else {
        Err(GwsError::Validation(format!(
            "Invalid message format '{}'. Allowed values: {}",
            value,
            VALID_MSG_FORMATS.join(", ")
        )))
    }
}

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

/// Rejects strings containing null bytes or ASCII control characters.
fn reject_control_chars(value: &str, flag_name: &str) -> Result<(), GwsError> {
    if value.bytes().any(|b| b < 0x20) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    // --- validate_msg_format ---

    #[test]
    fn test_valid_msg_formats() {
        for fmt in VALID_MSG_FORMATS {
            assert!(
                validate_msg_format(fmt).is_ok(),
                "expected '{fmt}' to be valid"
            );
        }
    }

    #[test]
    fn test_invalid_msg_format() {
        let err = validate_msg_format("FULL").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Invalid message format"), "got: {msg}");
        assert!(msg.contains("full, metadata, minimal, raw"));
    }

    #[test]
    fn test_empty_msg_format() {
        assert!(validate_msg_format("").is_err());
    }

    #[test]
    fn test_msg_format_injection_attempt() {
        assert!(validate_msg_format("full&extra=1").is_err());
    }

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
}

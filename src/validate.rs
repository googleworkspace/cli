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
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::io::Read;
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

/// Validates that `path` is a safe input file path for reading (e.g. `--upload`).
///
/// The path must be relative to CWD, must resolve within CWD, and must point
/// to an existing regular file.
pub fn validate_safe_input_file_path(path: &str) -> Result<PathBuf, GwsError> {
    validate_safe_read_file_path(path, "--upload")
}

/// Validates that `path` is a safe output file path for writing (e.g. `--output`).
///
/// The path must be relative to CWD and must resolve within CWD. Existing
/// directories are rejected.
pub fn validate_safe_output_file_path(path: &str) -> Result<PathBuf, GwsError> {
    validate_safe_write_file_path(path, "--output")
}

/// Validate and read an upload file in one operation.
///
/// This reduces the race window between path validation and file consumption by
/// reading from the validated path immediately.
pub fn read_safe_upload_file(path: &str) -> Result<Vec<u8>, GwsError> {
    let canonical = validate_safe_input_file_path(path)?;

    #[cfg(unix)]
    {
        let parent = canonical.parent().ok_or_else(|| {
            GwsError::Validation(format!(
                "Failed to resolve parent directory for --upload '{}'",
                path
            ))
        })?;
        let leaf = canonical.file_name().ok_or_else(|| {
            GwsError::Validation(format!(
                "Failed to resolve file name for --upload '{}'",
                path
            ))
        })?;

        let parent_dir = open_parent_dir_under_cwd(parent, "--upload", path)?;
        let mut file = open_child_no_follow(
            &parent_dir,
            leaf,
            libc::O_RDONLY | libc::O_NOFOLLOW,
            0,
            "--upload",
            path,
        )?;

        let meta = file.metadata().map_err(|e| {
            GwsError::Validation(format!(
                "Failed to inspect upload file '{}': {}",
                canonical.display(),
                e
            ))
        })?;
        if !meta.is_file() {
            return Err(GwsError::Validation(format!(
                "--upload '{}' must reference a regular file",
                path
            )));
        }

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|e| {
            GwsError::Validation(format!(
                "Failed to read upload file '{}': {}",
                canonical.display(),
                e
            ))
        })?;
        Ok(bytes)
    }

    #[cfg(not(unix))]
    {
        std::fs::read(&canonical).map_err(|e| {
            GwsError::Validation(format!(
                "Failed to read upload file '{}': {}",
                canonical.display(),
                e
            ))
        })
    }
}

/// Validate and open an output file in one operation.
///
/// Returns a trusted writable file handle and the canonical path used for
/// writing. Existing files are opened without truncation first so post-open
/// safety checks still run before bytes are written.
pub fn open_safe_output_file(path: &str) -> Result<(std::fs::File, PathBuf), GwsError> {
    let canonical = validate_safe_output_file_path(path)?;
    let existed_before_open = std::fs::symlink_metadata(&canonical).is_ok();

    #[cfg(unix)]
    {
        let parent = canonical.parent().ok_or_else(|| {
            GwsError::Validation(format!(
                "Failed to resolve parent directory for --output '{}'",
                path
            ))
        })?;
        let leaf = canonical.file_name().ok_or_else(|| {
            GwsError::Validation(format!(
                "Failed to resolve file name for --output '{}'",
                path
            ))
        })?;

        let parent_dir = open_parent_dir_under_cwd(parent, "--output", path)?;
        let flags = if existed_before_open {
            libc::O_WRONLY | libc::O_NOFOLLOW
        } else {
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW
        };
        let file = open_child_no_follow(&parent_dir, leaf, flags, 0o600, "--output", path)?;

        let meta = file.metadata().map_err(|e| {
            GwsError::Validation(format!(
                "Failed to inspect output file '{}': {}",
                canonical.display(),
                e
            ))
        })?;
        if !meta.is_file() {
            return Err(GwsError::Validation(format!(
                "--output '{}' is not a regular file",
                path
            )));
        }

        if existed_before_open {
            file.set_len(0).map_err(|e| {
                GwsError::Validation(format!(
                    "Failed to truncate output file '{}': {}",
                    canonical.display(),
                    e
                ))
            })?;
        }

        Ok((file, canonical))
    }

    #[cfg(not(unix))]
    {
        let mut opts = OpenOptions::new();
        opts.write(true);
        if existed_before_open {
            opts.create(false);
        } else {
            opts.create_new(true);
        }

        let file = opts.open(&canonical).map_err(|e| {
            GwsError::Validation(format!(
                "Failed to open output file '{}': {}",
                canonical.display(),
                e
            ))
        })?;

        let canonical_after_open = canonical.canonicalize().map_err(|e| {
            GwsError::Validation(format!(
                "Failed to resolve output file after open '{}': {}",
                canonical.display(),
                e
            ))
        })?;
        let cwd = std::env::current_dir().map_err(|e| {
            GwsError::Validation(format!("Failed to determine current directory: {e}"))
        })?;
        let canonical_cwd = cwd.canonicalize().map_err(|e| {
            GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
        })?;
        if !canonical_after_open.starts_with(&canonical_cwd) {
            return Err(GwsError::Validation(format!(
                "--output '{}' resolves outside current directory after open: '{}'",
                path,
                canonical_after_open.display()
            )));
        }

        let meta = file.metadata().map_err(|e| {
            GwsError::Validation(format!(
                "Failed to inspect output file '{}': {}",
                canonical_after_open.display(),
                e
            ))
        })?;
        if !meta.is_file() {
            return Err(GwsError::Validation(format!(
                "--output '{}' is not a regular file",
                path
            )));
        }

        if existed_before_open {
            file.set_len(0).map_err(|e| {
                GwsError::Validation(format!(
                    "Failed to truncate output file '{}': {}",
                    canonical_after_open.display(),
                    e
                ))
            })?;
        }

        Ok((file, canonical_after_open))
    }
}

#[cfg(unix)]
fn open_parent_dir_under_cwd(
    parent: &Path,
    flag_name: &str,
    original_path: &str,
) -> Result<std::fs::File, GwsError> {
    use std::os::unix::fs::MetadataExt;

    let dir = std::fs::File::open(parent).map_err(|e| {
        GwsError::Validation(format!(
            "Failed to open parent directory for {flag_name} '{}': {}",
            original_path, e
        ))
    })?;

    let dir_meta = dir.metadata().map_err(|e| {
        GwsError::Validation(format!(
            "Failed to inspect parent directory for {flag_name} '{}': {}",
            original_path, e
        ))
    })?;
    if !dir_meta.is_dir() {
        return Err(GwsError::Validation(format!(
            "Parent path for {flag_name} '{}' is not a directory",
            original_path
        )));
    }

    let cwd = std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))?;
    let canonical_cwd = cwd.canonicalize().map_err(|e| {
        GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
    })?;
    let canonical_parent = parent.canonicalize().map_err(|e| {
        GwsError::Validation(format!(
            "Failed to resolve parent directory for {flag_name} '{}': {}",
            original_path, e
        ))
    })?;
    if !canonical_parent.starts_with(&canonical_cwd) {
        return Err(GwsError::Validation(format!(
            "{flag_name} '{}' resolves outside current directory",
            original_path
        )));
    }

    let parent_meta = std::fs::metadata(&canonical_parent).map_err(|e| {
        GwsError::Validation(format!(
            "Failed to inspect resolved parent directory for {flag_name} '{}': {}",
            original_path, e
        ))
    })?;
    if dir_meta.dev() != parent_meta.dev() || dir_meta.ino() != parent_meta.ino() {
        return Err(GwsError::Validation(format!(
            "Detected concurrent filesystem change while resolving {flag_name} '{}'",
            original_path
        )));
    }

    Ok(dir)
}

#[cfg(unix)]
fn open_child_no_follow(
    parent_dir: &std::fs::File,
    leaf: &std::ffi::OsStr,
    flags: libc::c_int,
    mode: libc::mode_t,
    flag_name: &str,
    original_path: &str,
) -> Result<std::fs::File, GwsError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let leaf_c = CString::new(leaf.as_bytes()).map_err(|_| {
        GwsError::Validation(format!(
            "{flag_name} '{}' contains an invalid path segment",
            original_path
        ))
    })?;

    let fd = unsafe {
        libc::openat(
            parent_dir.as_raw_fd(),
            leaf_c.as_ptr(),
            flags,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(GwsError::Validation(format!(
            "Failed to open {flag_name} '{}': {}",
            original_path,
            std::io::Error::last_os_error()
        )));
    }

    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

fn validate_safe_read_file_path(path: &str, flag_name: &str) -> Result<PathBuf, GwsError> {
    reject_control_chars(path, flag_name)?;

    let input_path = Path::new(path);
    if input_path.as_os_str().is_empty() {
        return Err(GwsError::Validation(format!(
            "{flag_name} must not be empty"
        )));
    }
    if input_path.is_absolute() {
        return Err(GwsError::Validation(format!(
            "{flag_name} must be a relative path, got absolute path '{}'",
            path
        )));
    }
    if input_path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(GwsError::Validation(format!(
            "{flag_name} must not contain path traversal ('..'): {}",
            path
        )));
    }

    let cwd = std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))?;
    let resolved = cwd.join(input_path);
    let canonical = resolved.canonicalize().map_err(|e| {
        GwsError::Validation(format!("Failed to resolve {flag_name} '{}': {e}", path))
    })?;

    let canonical_cwd = cwd.canonicalize().map_err(|e| {
        GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
    })?;
    if !canonical.starts_with(&canonical_cwd) {
        return Err(GwsError::Validation(format!(
            "{flag_name} '{}' resolves to '{}' which is outside the current directory",
            path,
            canonical.display()
        )));
    }

    if !canonical.is_file() {
        return Err(GwsError::Validation(format!(
            "{flag_name} '{}' must reference a file",
            path
        )));
    }

    Ok(canonical)
}

fn validate_safe_write_file_path(path: &str, flag_name: &str) -> Result<PathBuf, GwsError> {
    reject_control_chars(path, flag_name)?;

    let output_path = Path::new(path);
    if output_path.as_os_str().is_empty() {
        return Err(GwsError::Validation(format!(
            "{flag_name} must not be empty"
        )));
    }
    if output_path.is_absolute() {
        return Err(GwsError::Validation(format!(
            "{flag_name} must be a relative path, got absolute path '{}'",
            path
        )));
    }
    if output_path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(GwsError::Validation(format!(
            "{flag_name} must not contain path traversal ('..'): {}",
            path
        )));
    }

    let cwd = std::env::current_dir()
        .map_err(|e| GwsError::Validation(format!("Failed to determine current directory: {e}")))?;
    let resolved = cwd.join(output_path);
    let canonical = if std::fs::symlink_metadata(&resolved).is_ok() {
        resolved.canonicalize().map_err(|e| {
            GwsError::Validation(format!("Failed to resolve {flag_name} '{}': {e}", path))
        })?
    } else {
        normalize_non_existing(&resolved)?
    };

    let canonical_cwd = cwd.canonicalize().map_err(|e| {
        GwsError::Validation(format!("Failed to canonicalize current directory: {e}"))
    })?;
    if !canonical.starts_with(&canonical_cwd) {
        return Err(GwsError::Validation(format!(
            "{flag_name} '{}' resolves to '{}' which is outside the current directory",
            path,
            canonical.display()
        )));
    }

    if canonical.exists() && !canonical.is_file() {
        return Err(GwsError::Validation(format!(
            "{flag_name} '{}' must reference a regular file path",
            path
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

/// Resolves a path that may not exist yet by canonicalizing the longest
/// existing prefix (including symlinks) and appending the remaining components.
fn normalize_non_existing(path: &Path) -> Result<PathBuf, GwsError> {
    let mut resolved = PathBuf::new();
    let mut remaining = Vec::new();

    // Walk backwards until we find a component that exists
    let mut current = path.to_path_buf();
    loop {
        if std::fs::symlink_metadata(&current).is_ok() {
            resolved = current.canonicalize().map_err(|e| {
                GwsError::Validation(format!(
                    "Failed to canonicalize path '{}': {e}",
                    current.display()
                ))
            })?;
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

    // --- validate_safe_input_file_path ---

    #[test]
    #[serial]
    fn test_input_file_path_valid_relative_file() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        fs::create_dir_all(canonical_dir.join("sub")).unwrap();
        fs::write(canonical_dir.join("sub").join("file.txt"), b"ok").unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let result = validate_safe_input_file_path("sub/file.txt");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[test]
    fn test_input_file_path_rejects_absolute() {
        assert!(validate_safe_input_file_path("/tmp/evil").is_err());
    }

    #[test]
    #[serial]
    fn test_input_file_path_rejects_traversal() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let result = validate_safe_input_file_path("../secret.txt");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    #[test]
    #[serial]
    fn test_input_file_path_rejects_symlink_escape() {
        let cwd_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let canonical_cwd = cwd_dir.path().canonicalize().unwrap();
        let canonical_outside = outside_dir.path().canonicalize().unwrap();
        fs::write(canonical_outside.join("secret.txt"), b"secret").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&canonical_outside, canonical_cwd.join("sneaky")).unwrap();
        #[cfg(windows)]
        return;

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_cwd).unwrap();
        let result = validate_safe_input_file_path("sneaky/secret.txt");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("outside the current directory"));
    }

    #[test]
    #[serial]
    fn test_read_safe_upload_file_reads_valid_file() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        fs::write(canonical_dir.join("input.txt"), b"hello").unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let result = read_safe_upload_file("input.txt");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert_eq!(result.unwrap(), b"hello");
    }

    // --- validate_safe_output_file_path ---

    #[test]
    #[serial]
    fn test_output_file_path_valid_nested_under_cwd() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let result = validate_safe_output_file_path("downloads/out.bin");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[test]
    fn test_output_file_path_rejects_absolute() {
        assert!(validate_safe_output_file_path("/tmp/out.bin").is_err());
    }

    #[test]
    #[serial]
    fn test_output_file_path_rejects_traversal() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let result = validate_safe_output_file_path("../out.bin");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    #[test]
    #[serial]
    fn test_output_file_path_rejects_symlink_escape_parent() {
        let cwd_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let canonical_cwd = cwd_dir.path().canonicalize().unwrap();
        let canonical_outside = outside_dir.path().canonicalize().unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&canonical_outside, canonical_cwd.join("sneaky_out")).unwrap();
        #[cfg(windows)]
        return;

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_cwd).unwrap();
        let result = validate_safe_output_file_path("sneaky_out/out.bin");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("outside the current directory"));
    }

    #[test]
    #[serial]
    fn test_output_file_path_rejects_broken_symlink_prefix() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(
            "/tmp/definitely-missing-target",
            canonical_dir.join("broken"),
        )
        .unwrap();
        #[cfg(windows)]
        return;

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let result = validate_safe_output_file_path("broken/out.bin");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
    }

    #[test]
    #[serial]
    #[cfg(unix)]
    fn test_output_file_path_rejects_fifo_target() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let fifo = canonical_dir.join("out.fifo");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();

        let status = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) };
        assert_eq!(
            status,
            0,
            "mkfifo failed: {}",
            std::io::Error::last_os_error()
        );

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let result = validate_safe_output_file_path("out.fifo");
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn test_open_safe_output_file_creates_new_file() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let (mut file, path) = open_safe_output_file("created.bin").unwrap();
        std::io::Write::write_all(&mut file, b"abc").unwrap();
        drop(file);
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(path.ends_with("created.bin"));
        assert_eq!(fs::read(path).unwrap(), b"abc");
    }

    #[test]
    #[serial]
    fn test_open_safe_output_file_truncates_existing_file() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        fs::write(canonical_dir.join("existing.bin"), b"old").unwrap();

        let saved_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();
        let (mut file, path) = open_safe_output_file("existing.bin").unwrap();
        std::io::Write::write_all(&mut file, b"new").unwrap();
        drop(file);
        std::env::set_current_dir(&saved_cwd).unwrap();

        assert!(path.ends_with("existing.bin"));
        assert_eq!(fs::read(path).unwrap(), b"new");
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
}

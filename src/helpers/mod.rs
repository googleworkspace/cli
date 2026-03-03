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

use crate::error::GwsError;
use clap::{ArgMatches, Command};
use std::future::Future;
use std::pin::Pin;
pub mod calendar;
pub mod chat;
pub mod docs;
pub mod drive;
pub mod events;
pub mod gmail;
pub mod modelarmor;
pub mod script;
pub mod sheets;
pub mod workflows;

/// A trait for service-specific CLI helpers that inject custom commands.
pub trait Helper: Send + Sync {
    /// Injects subcommands into the service command.
    fn inject_commands(&self, cmd: Command, doc: &crate::discovery::RestDescription) -> Command;

    /// Attempts to handle a command. Returns Ok(Some(())) if handled,
    /// Ok(None) if not handled (should fall back to dynamic dispatch),
    /// or Err if handled but failed.
    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize_config: &'a modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>>;

    /// If true, only helper commands are shown (discovery-generated commands are suppressed).
    fn helper_only(&self) -> bool {
        false
    }
}

pub fn get_helper(service: &str) -> Option<Box<dyn Helper>> {
    match service {
        "gmail" => Some(Box::new(gmail::GmailHelper)),
        "sheets" => Some(Box::new(sheets::SheetsHelper)),
        "docs" => Some(Box::new(docs::DocsHelper)),
        "chat" => Some(Box::new(chat::ChatHelper)),
        "drive" => Some(Box::new(drive::DriveHelper)),
        "calendar" => Some(Box::new(calendar::CalendarHelper)),
        "script" | "apps-script" => Some(Box::new(script::ScriptHelper)),
        "workspaceevents" => Some(Box::new(events::EventsHelper)),
        "modelarmor" => Some(Box::new(modelarmor::ModelArmorHelper)),
        "workflow" => Some(Box::new(workflows::WorkflowHelper)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// URL safety helpers
// ---------------------------------------------------------------------------

/// Percent-encode a value for use as a single URL path segment (e.g., file ID,
/// calendar ID, message ID). All non-alphanumeric characters are encoded.
pub(crate) fn encode_path_segment(s: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

/// Validate a multi-segment resource name (e.g., `spaces/ABC`, `subscriptions/123`).
/// Rejects path traversal and control characters while preserving the intentional
/// `/`-delimited structure. Returns the validated name or an error with a clear
/// message suitable for LLM callers.
pub(crate) fn validate_resource_name(s: &str) -> Result<&str, GwsError> {
    if s.is_empty() {
        return Err(GwsError::Validation(
            "Resource name must not be empty".to_string(),
        ));
    }
    if s.contains("..") {
        return Err(GwsError::Validation(format!(
            "Resource name must not contain '..': {s}"
        )));
    }
    if s.contains('\0') || s.chars().any(|c| c.is_control()) {
        return Err(GwsError::Validation(format!(
            "Resource name contains invalid characters: {s}"
        )));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // LLMs might append query strings to resource names — this is allowed
        // by validation since `?` is not a traversal/control char, but the
        // API will reject it with a clear error.
        assert!(validate_resource_name("spaces/ABC?key=val").is_ok());
    }

    #[test]
    fn test_validate_resource_name_error_messages_are_clear() {
        let err = validate_resource_name("").unwrap_err();
        assert!(err.to_string().contains("must not be empty"));

        let err = validate_resource_name("../bad").unwrap_err();
        assert!(err.to_string().contains("must not contain '..'"));

        let err = validate_resource_name("bad\0id").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
    }
}

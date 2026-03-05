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

//! Multi-account registry for `gws`.
//!
//! Manages `~/.config/gws/accounts.json` which maps email addresses to
//! credential files and tracks the default account.

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

/// On-disk representation of `accounts.json`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AccountsRegistry {
    /// Email of the default account, or `None` if no default is set.
    pub default: Option<String>,
    /// Map from normalised email → account metadata.
    pub accounts: BTreeMap<String, AccountMeta>,
}

/// Per-account metadata stored in the registry.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountMeta {
    /// ISO-8601 timestamp of when this account was added.
    pub added: String,
}

// ---------------------------------------------------------------------------
// Email normalisation & base64 helpers
// ---------------------------------------------------------------------------

/// Normalise an email address: trim whitespace and lowercase.
///
/// Google treats email addresses as case-insensitive, so
/// `User@Gmail.COM` and `user@gmail.com` must map to the same
/// credential file and registry entry.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Encode a normalised email to a URL-safe Base64 string (no padding).
///
/// This is used as the unique key in credential/token-cache filenames
/// (e.g. `credentials.<b64>.enc`) to avoid filesystem issues with `@`
/// and `.` characters across operating systems.
pub fn email_to_b64(email: &str) -> String {
    URL_SAFE_NO_PAD.encode(email.as_bytes())
}

// ---------------------------------------------------------------------------
// Registry I/O
// ---------------------------------------------------------------------------

/// Path to `accounts.json` inside the config directory.
pub fn accounts_path() -> PathBuf {
    crate::auth_commands::config_dir().join("accounts.json")
}

/// Load the accounts registry from disk. Returns `None` if the file does not
/// exist, and an error if it exists but cannot be parsed.
pub fn load_accounts() -> anyhow::Result<Option<AccountsRegistry>> {
    let path = accounts_path();
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path)?;
    let registry: AccountsRegistry = serde_json::from_str(&data)?;
    Ok(Some(registry))
}

/// Persist the accounts registry to disk with `0o600` permissions.
pub fn save_accounts(registry: &AccountsRegistry) -> anyhow::Result<()> {
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    let json = serde_json::to_string_pretty(registry)?;
    crate::fs_util::atomic_write(&path, json.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to write accounts.json: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Registry mutations
// ---------------------------------------------------------------------------

/// Return the default account email, if one is set.
pub fn get_default(registry: &AccountsRegistry) -> Option<&str> {
    registry.default.as_deref()
}

/// Set the default account. Returns an error if the email is not registered.
pub fn set_default(registry: &mut AccountsRegistry, email: &str) -> anyhow::Result<()> {
    let normalised = normalize_email(email);
    if !registry.accounts.contains_key(&normalised) {
        anyhow::bail!(
            "Account '{}' not found. Run 'gws auth login' to add it.",
            normalised
        );
    }
    registry.default = Some(normalised);
    Ok(())
}

/// Register a new account (or update its metadata if it already exists).
/// If this is the first account, it becomes the default automatically.
pub fn add_account(registry: &mut AccountsRegistry, email: &str) {
    let normalised = normalize_email(email);
    let meta = AccountMeta {
        added: chrono::Utc::now().to_rfc3339(),
    };
    registry.accounts.insert(normalised.clone(), meta);
    if registry.default.is_none() || registry.accounts.len() == 1 {
        registry.default = Some(normalised);
    }
}

/// Remove an account from the registry.
///
/// If the removed account was the default, the default is auto-promoted to
/// the next available account (or set to `None` if no accounts remain).
pub fn remove_account(registry: &mut AccountsRegistry, email: &str) {
    let normalised = normalize_email(email);
    registry.accounts.remove(&normalised);

    // Handle dangling default
    if registry.default.as_deref() == Some(&normalised) {
        registry.default = registry.accounts.keys().next().cloned();
    }
}

/// List all registered account emails.
pub fn list_accounts(registry: &AccountsRegistry) -> Vec<&str> {
    registry.accounts.keys().map(|s| s.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_normalization() {
        assert_eq!(normalize_email("  User@Gmail.COM  "), "user@gmail.com");
        assert_eq!(normalize_email("WORK@Corp.com"), "work@corp.com");
        assert_eq!(normalize_email("simple@example.com"), "simple@example.com");
    }

    #[test]
    fn test_email_to_b64_no_pad() {
        let encoded = email_to_b64("user@gmail.com");
        // Must not contain +, /, or =
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
        // Must be non-empty and deterministic
        assert!(!encoded.is_empty());
        assert_eq!(encoded, email_to_b64("user@gmail.com"));
    }

    #[test]
    fn test_email_case_produces_same_b64() {
        let a = email_to_b64(&normalize_email("User@Gmail.COM"));
        let b = email_to_b64(&normalize_email("user@gmail.com"));
        assert_eq!(
            a, b,
            "Case-different emails should produce the same b64 after normalization"
        );
    }

    #[test]
    fn test_accounts_json_round_trip() {
        let mut registry = AccountsRegistry::default();
        assert!(registry.accounts.is_empty());
        assert!(registry.default.is_none());

        // Add first account → auto-default
        add_account(&mut registry, "first@example.com");
        assert_eq!(registry.default.as_deref(), Some("first@example.com"));
        assert_eq!(list_accounts(&registry), vec!["first@example.com"]);

        // Add second account → default unchanged
        add_account(&mut registry, "second@example.com");
        assert_eq!(registry.default.as_deref(), Some("first@example.com"));
        assert_eq!(list_accounts(&registry).len(), 2);

        // Set default
        set_default(&mut registry, "second@example.com").unwrap();
        assert_eq!(registry.default.as_deref(), Some("second@example.com"));

        // Set default to unknown → error
        let err = set_default(&mut registry, "unknown@example.com");
        assert!(err.is_err());

        // Remove default account → auto-promote
        remove_account(&mut registry, "second@example.com");
        assert!(registry.default.is_some()); // promoted to first
        assert_eq!(list_accounts(&registry), vec!["first@example.com"]);

        // Remove last account → default is None
        remove_account(&mut registry, "first@example.com");
        assert!(registry.default.is_none());
        assert!(registry.accounts.is_empty());
    }

    #[test]
    fn test_serde_round_trip() {
        let mut registry = AccountsRegistry::default();
        add_account(&mut registry, "test@example.com");

        let json = serde_json::to_string_pretty(&registry).unwrap();
        let parsed: AccountsRegistry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.default.as_deref(), Some("test@example.com"));
        assert!(parsed.accounts.contains_key("test@example.com"));
    }
}

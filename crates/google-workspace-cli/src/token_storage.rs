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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use yup_oauth2::storage::{TokenInfo, TokenStorage, TokenStorageError};

use crate::output::sanitize_for_terminal;

/// A custom token storage implementation for `yup-oauth2` that encrypts
/// the cached tokens at rest using AES-256-GCM encryption.
///
/// Cached tokens can be bound to the credential that minted them (see
/// [`EncryptedTokenStorage::with_credential_fingerprint`]). yup-oauth2 stores
/// the refresh token alongside each cached access token and refreshes expired
/// entries with that stored refresh token, so an unbound cache keeps serving
/// the previous account (or the previous scope grant) after the credentials
/// change. Binding the entries to a fingerprint of the credential makes a
/// cache written for one credential invisible to every other credential.
pub struct EncryptedTokenStorage {
    file_path: PathBuf,
    /// Fingerprint of the credential this storage serves; entries written for
    /// any other credential are ignored on read and pruned on write.
    credential_fingerprint: Option<String>,
    // Add memory cache since TokenStorage getters can be called frequently
    cache: Arc<Mutex<Option<HashMap<String, TokenInfo>>>>,
}

/// Separates the credential fingerprint from the scope list in cache keys.
/// Scopes are URLs or short names and never contain this character.
const FINGERPRINT_SEPARATOR: char = '|';

impl EncryptedTokenStorage {
    pub fn new(path: PathBuf) -> Self {
        Self {
            file_path: path,
            credential_fingerprint: None,
            cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Binds cached tokens to the credential identified by `fingerprint`
    /// (an opaque, non-secret digest; see `auth::credential_fingerprint`).
    pub fn with_credential_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.credential_fingerprint = Some(fingerprint.into());
        self
    }

    async fn load_from_disk(&self) -> HashMap<String, TokenInfo> {
        let data = match tokio::fs::read(&self.file_path).await {
            Ok(d) => d,
            Err(_) => return HashMap::new(), // File doesn't exist yet — normal on first run
        };

        let decrypted = match crate::credential_store::decrypt(&data) {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "warning: failed to decrypt token cache ({}): {e:#}",
                    self.file_path.display()
                );
                eprintln!("hint: you may need to re-authenticate with `gws auth login`");
                return HashMap::new();
            }
        };

        let json = match String::from_utf8(decrypted) {
            Ok(j) => j,
            Err(e) => {
                eprintln!(
                    "warning: token cache contains invalid UTF-8: {}",
                    sanitize_for_terminal(&e.to_string())
                );
                return HashMap::new();
            }
        };

        match serde_json::from_str(&json) {
            Ok(map) => map,
            Err(e) => {
                eprintln!(
                    "warning: failed to parse token cache JSON: {}",
                    sanitize_for_terminal(&e.to_string())
                );
                HashMap::new()
            }
        }
    }

    async fn save_to_disk(&self, map: &HashMap<String, TokenInfo>) -> anyhow::Result<()> {
        let json = serde_json::to_string(map)?;
        let encrypted = crate::credential_store::encrypt(json.as_bytes())?;

        if let Some(parent) = self.file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create token directory '{}': {}",
                    sanitize_for_terminal(&parent.display().to_string()),
                    e
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to set permissions on token directory '{}': {}",
                            sanitize_for_terminal(&parent.display().to_string()),
                            e
                        )
                    })?;
            }
        }

        // Write atomically via a sibling .tmp file + rename.
        crate::fs_util::atomic_write_async(&self.file_path, encrypted.as_slice()).await?;

        Ok(())
    }

    // Helper to join scopes consistently for cache keys
    fn scope_key(scopes: &[&str]) -> String {
        let mut s: Vec<&str> = scopes.to_vec();
        s.sort_unstable();
        s.dedup();
        s.join(" ")
    }

    /// Cache key for `scopes`, prefixed with the credential fingerprint when
    /// the storage is bound to a credential.
    fn cache_key(&self, scopes: &[&str]) -> String {
        let scopes = Self::scope_key(scopes);
        match &self.credential_fingerprint {
            Some(fp) => format!("{fp}{FINGERPRINT_SEPARATOR}{scopes}"),
            None => scopes,
        }
    }

    /// Whether a stored entry belongs to this storage's credential. Unbound
    /// storage owns only unprefixed (legacy) keys; bound storage owns only keys
    /// carrying its own fingerprint.
    fn owns_key(&self, key: &str) -> bool {
        match &self.credential_fingerprint {
            Some(fp) => key
                .strip_prefix(fp.as_str())
                .is_some_and(|rest| rest.starts_with(FINGERPRINT_SEPARATOR)),
            None => !key.contains(FINGERPRINT_SEPARATOR),
        }
    }
}

#[async_trait::async_trait]
impl TokenStorage for EncryptedTokenStorage {
    async fn set(&self, scopes: &[&str], token: TokenInfo) -> Result<(), TokenStorageError> {
        let mut map_lock = self.cache.lock().await;

        // Initialize cache if this is the first write
        if map_lock.is_none() {
            *map_lock = Some(self.load_from_disk().await);
        }

        if let Some(map) = map_lock.as_mut() {
            // Drop entries minted for any other credential (a previous login,
            // another account, or the legacy unbound format): they can never
            // be served again and still hold that credential's refresh token.
            map.retain(|key, _| self.owns_key(key));
            map.insert(self.cache_key(scopes), token);
            self.save_to_disk(map)
                .await
                .map_err(|e| TokenStorageError::Other(std::borrow::Cow::Owned(e.to_string())))?;
        }

        Ok(())
    }

    async fn get(&self, scopes: &[&str]) -> Option<TokenInfo> {
        let mut map_lock = self.cache.lock().await;

        if map_lock.is_none() {
            *map_lock = Some(self.load_from_disk().await);
        }

        if let Some(map) = map_lock.as_ref() {
            let key = self.cache_key(scopes);
            if let Some(token) = map.get(&key) {
                return Some(token.clone());
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_encrypted_token_storage_new() {
        let path = PathBuf::from("/fake/path/to/token.json");
        let storage = EncryptedTokenStorage::new(path.clone());

        assert_eq!(storage.file_path, path);

        let cache_lock = storage.cache.lock().await;
        assert!(cache_lock.is_none());
    }

    fn token(access: &str) -> TokenInfo {
        TokenInfo {
            access_token: Some(access.to_string()),
            refresh_token: Some(format!("refresh-for-{access}")),
            expires_at: None,
            id_token: None,
        }
    }

    #[test]
    fn test_cache_key_is_scope_order_independent_and_prefixed() {
        let unbound = EncryptedTokenStorage::new(PathBuf::from("/x"));
        assert_eq!(unbound.cache_key(&["b", "a", "a"]), "a b");

        let bound =
            EncryptedTokenStorage::new(PathBuf::from("/x")).with_credential_fingerprint("fp1");
        assert_eq!(bound.cache_key(&["b", "a"]), "fp1|a b");
    }

    #[test]
    fn test_owns_key() {
        let unbound = EncryptedTokenStorage::new(PathBuf::from("/x"));
        assert!(unbound.owns_key("a b"));
        assert!(!unbound.owns_key("fp1|a b"));

        let bound =
            EncryptedTokenStorage::new(PathBuf::from("/x")).with_credential_fingerprint("fp1");
        assert!(bound.owns_key("fp1|a b"));
        assert!(!bound.owns_key("fp2|a b"));
        assert!(!bound.owns_key("fp1x|a b"));
        assert!(!bound.owns_key("a b"));
    }

    /// Regression test for #780 / #572: a token cached for one credential must
    /// never be served to another one sharing the same cache file.
    #[tokio::test]
    #[serial_test::serial]
    async fn test_cache_is_not_shared_between_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let _config = ConfigDirGuard::set(dir.path());
        let path = dir.path().join("token_cache.json");
        let scopes = ["https://www.googleapis.com/auth/drive"];

        let account_a =
            EncryptedTokenStorage::new(path.clone()).with_credential_fingerprint("account-a");
        account_a.set(&scopes, token("token-a")).await.unwrap();
        assert_eq!(
            account_a
                .get(&scopes)
                .await
                .unwrap()
                .access_token
                .as_deref(),
            Some("token-a")
        );

        // A fresh process for account B reads the same file from disk.
        let account_b =
            EncryptedTokenStorage::new(path.clone()).with_credential_fingerprint("account-b");
        assert!(account_b.get(&scopes).await.is_none());

        // Writing B's token prunes A's entry (and its refresh token) from disk.
        account_b.set(&scopes, token("token-b")).await.unwrap();
        let reread_a =
            EncryptedTokenStorage::new(path.clone()).with_credential_fingerprint("account-a");
        assert!(reread_a.get(&scopes).await.is_none());
        let reread_b = EncryptedTokenStorage::new(path).with_credential_fingerprint("account-b");
        assert_eq!(
            reread_b.get(&scopes).await.unwrap().access_token.as_deref(),
            Some("token-b")
        );
    }

    /// Caches written before entries were bound to a credential use bare
    /// scope keys; bound storage must ignore them rather than trust them.
    #[tokio::test]
    #[serial_test::serial]
    async fn test_bound_storage_ignores_legacy_unbound_entries() {
        let dir = tempfile::tempdir().unwrap();
        let _config = ConfigDirGuard::set(dir.path());
        let path = dir.path().join("token_cache.json");
        let scopes = ["https://www.googleapis.com/auth/drive"];

        let legacy = EncryptedTokenStorage::new(path.clone());
        legacy.set(&scopes, token("legacy")).await.unwrap();

        let bound = EncryptedTokenStorage::new(path).with_credential_fingerprint("account-a");
        assert!(bound.get(&scopes).await.is_none());
    }

    /// Points the encryption key file at a temp dir for the test's duration.
    struct ConfigDirGuard(Option<std::ffi::OsString>);

    impl ConfigDirGuard {
        fn set(dir: &std::path::Path) -> Self {
            let original = std::env::var_os("GOOGLE_WORKSPACE_CLI_CONFIG_DIR");
            unsafe { std::env::set_var("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", dir) };
            Self(original)
        }
    }

    impl Drop for ConfigDirGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => unsafe { std::env::set_var("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", v) },
                None => unsafe { std::env::remove_var("GOOGLE_WORKSPACE_CLI_CONFIG_DIR") },
            }
        }
    }
}

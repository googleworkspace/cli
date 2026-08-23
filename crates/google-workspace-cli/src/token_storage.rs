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
pub struct EncryptedTokenStorage {
    file_path: PathBuf,
    // Add memory cache since TokenStorage getters can be called frequently
    cache: Arc<Mutex<Option<HashMap<String, TokenInfo>>>>,
}

impl EncryptedTokenStorage {
    pub fn new(path: PathBuf) -> Self {
        Self {
            file_path: path,
            cache: Arc::new(Mutex::new(None)),
        }
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
    fn cache_key(scopes: &[&str]) -> String {
        let mut s: Vec<&str> = scopes.to_vec();
        s.sort_unstable();
        s.dedup();
        s.join(" ")
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
            map.insert(Self::cache_key(scopes), token);
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
            let key = Self::cache_key(scopes);
            if let Some(token) = map.get(&key) {
                // yup-oauth2 treats a missing expiry as "never expires". Google access tokens
                // are short-lived, so reusing such an entry can cause permanent 401 responses.
                // Treat it as a cache miss and let the authenticator fetch a fresh token.
                if token.expires_at.is_some() {
                    return Some(token.clone());
                }
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

    #[tokio::test]
    async fn test_get_ignores_cached_token_without_expiry() {
        let storage = EncryptedTokenStorage::new(PathBuf::from("/unused/token.json"));
        let scopes = ["scope-a", "scope-b"];
        let token = TokenInfo {
            access_token: Some("stale-access-token".into()),
            refresh_token: Some("refresh-token".into()),
            expires_at: None,
            id_token: None,
        };

        *storage.cache.lock().await = Some(HashMap::from([(
            EncryptedTokenStorage::cache_key(&scopes),
            token,
        )]));

        assert!(storage.get(&scopes).await.is_none());
    }

    #[tokio::test]
    async fn test_get_returns_cached_token_with_known_expiry() {
        let storage = EncryptedTokenStorage::new(PathBuf::from("/unused/token.json"));
        let scopes = ["scope-a"];
        let token: TokenInfo = serde_json::from_str(
            r#"{"access_token":"access-token","refresh_token":"refresh-token","expires_at":[2026,43,19,44,15,0,0,0,0],"id_token":null}"#,
        )
        .unwrap();

        *storage.cache.lock().await = Some(HashMap::from([(
            EncryptedTokenStorage::cache_key(&scopes),
            token.clone(),
        )]));

        assert_eq!(storage.get(&scopes).await, Some(token));
    }
}

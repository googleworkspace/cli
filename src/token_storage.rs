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

/// A custom token storage implementation for `yup-oauth2` that encrypts
/// the cached tokens at rest using the AES key derived from the OS keyring.
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
        if let Ok(data) = tokio::fs::read(&self.file_path).await {
            if let Ok(decrypted) = crate::credential_store::decrypt(&data) {
                if let Ok(json) = String::from_utf8(decrypted) {
                    if let Ok(map) = serde_json::from_str(&json) {
                        return map;
                    }
                }
            }
        }
        HashMap::new()
    }

    async fn save_to_disk(&self, map: &HashMap<String, TokenInfo>) -> anyhow::Result<()> {
        let json = serde_json::to_string(map)?;
        let encrypted = crate::credential_store::encrypt(json.as_bytes())?;

        if let Some(parent) = self.file_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
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
            // First look for exact match
            let key = Self::cache_key(scopes);
            if let Some(token) = map.get(&key) {
                return Some(token.clone());
            }

            // Fallback: check if we have a superset of the scopes (simplistic check compared to yup-oauth2 internal, but functional for this CLI)
            for (cached_key, token) in map.iter() {
                let cached_scopes: Vec<&str> = cached_key.split(' ').collect();
                if scopes.iter().all(|s| cached_scopes.contains(s)) {
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
}

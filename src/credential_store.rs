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

use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Nonce};

use keyring::Entry;
use rand::RngCore;


static KEY: tokio::sync::OnceCell<[u8; 32]> = tokio::sync::OnceCell::const_new();

/// Returns the encryption key derived from the OS keyring, or falls back to a local file.
/// Generates a random 256-bit key and stores it securely if it doesn't exist.
async fn get_or_create_key() -> anyhow::Result<[u8; 32]> {
    let key = KEY.get_or_try_init(generate_key_logic).await?;
    Ok(*key)
}

async fn generate_key_logic() -> anyhow::Result<[u8; 32]> {
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".to_string());

    let key_file = crate::auth_commands::config_dir().await.join(".encryption_key");

    let profile = crate::auth_commands::get_active_profile().await;
    let service_name = match profile.as_deref() {
        Some("default") | None => "gws-cli".to_string(),
        Some(name) => format!("gws-cli-{}", name),
    };

    let entry = Entry::new(&service_name, &username);

    if let Ok(entry) = entry {
        match entry.get_password() {
            Ok(b64_key) => {
                use base64::{engine::general_purpose::STANDARD, Engine as _};
                if let Ok(decoded) = STANDARD.decode(&b64_key) {
                    if decoded.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&decoded);
                        return Ok(arr);
                    }
                }
            }
            Err(keyring::Error::NoEntry) => {
                use base64::{engine::general_purpose::STANDARD, Engine as _};

                // If keyring is empty, prefer a persisted local key first.
                if tokio::fs::metadata(&key_file).await.is_ok() {
                    if let Ok(b64_key) = tokio::fs::read_to_string(&key_file).await {
                        if let Ok(decoded) = STANDARD.decode(b64_key.trim()) {
                            if decoded.len() == 32 {
                                let mut arr = [0u8; 32];
                                arr.copy_from_slice(&decoded);
                                // Best effort: repopulate keyring for future runs.
                                let _ = entry.set_password(&b64_key);
                                return Ok(arr);
                            }
                        }
                    }
                }

                // Generate a random 32-byte key and persist it locally as a stable fallback.
                let mut key = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut key);
                let b64_key = STANDARD.encode(key);

                if let Some(parent) = key_file.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let perms_result = async {
                            let mut perms = tokio::fs::metadata(parent).await?.permissions();
                            perms.set_mode(0o700);
                            tokio::fs::set_permissions(parent, perms).await
                        }
                        .await;
                        if let Err(e) = perms_result {
                            eprintln!(
                                "Warning: failed to set secure permissions on key directory: {e}"
                            );
                        }
                    }
                }

                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut std_options = std::fs::OpenOptions::new();
                    std_options.write(true).create(true).truncate(true).mode(0o600);
                    let options: tokio::fs::OpenOptions = std_options.into();
                    if let Ok(mut file) = options.open(&key_file).await {
                        use tokio::io::AsyncWriteExt;
                        let _ = file.write_all(b64_key.as_bytes()).await;
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = tokio::fs::write(&key_file, &b64_key).await;
                }

                // Best effort: also store in keyring when available.
                let _ = entry.set_password(&b64_key);

                return Ok(key);
            }
            Err(e) => {
                eprintln!("Warning: keyring access failed, falling back to file storage: {e}");
            }
        }
    }

    // Fallback: Local file `.encryption_key`

    if tokio::fs::metadata(&key_file).await.is_ok() {
        if let Ok(b64_key) = tokio::fs::read_to_string(&key_file).await {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            if let Ok(decoded) = STANDARD.decode(b64_key.trim()) {
                if decoded.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&decoded);
                    return Ok(arr);
                }
            }
        }
    }

    // Generate new key and save to local file
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let b64_key = STANDARD.encode(key);

    if let Some(parent) = key_file.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms_result = async {
                let mut perms = tokio::fs::metadata(parent).await?.permissions();
                perms.set_mode(0o700);
                tokio::fs::set_permissions(parent, perms).await
            }
            .await;
            if let Err(e) = perms_result {
                eprintln!("Warning: failed to set secure permissions on key directory: {e}");
            }
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut std_options = std::fs::OpenOptions::new();
        std_options.write(true).create(true).truncate(true).mode(0o600);
        let options: tokio::fs::OpenOptions = std_options.into();
        if let Ok(mut file) = options.open(&key_file).await {
            use tokio::io::AsyncWriteExt;
            let _ = file.write_all(b64_key.as_bytes()).await;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::fs::write(&key_file, b64_key).await;
    }

    Ok(key)
}

/// Encrypts plaintext bytes using AES-256-GCM with a machine-derived key.
/// Returns nonce (12 bytes) || ciphertext.
pub async fn encrypt(plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let key = get_or_create_key().await?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {e}"))?;

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {e}"))?;

    // Prepend nonce to ciphertext
    let mut result = nonce.to_vec();
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypts data produced by `encrypt()`.
pub async fn decrypt(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    if data.len() < 12 {
        anyhow::bail!("Encrypted data too short");
    }

    let key = get_or_create_key().await?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {e}"))?;

    let nonce = Nonce::from_slice(&data[..12]);
    let plaintext = cipher.decrypt(nonce, &data[12..]).map_err(|_| {
        anyhow::anyhow!(
            "Decryption failed. Credentials may have been created on a different machine. \
                 Run `gws auth logout` and `gws auth login` to re-authenticate."
        )
    })?;

    Ok(plaintext)
}

/// Returns the path for encrypted credentials.
pub async fn encrypted_credentials_path() -> PathBuf {
    crate::auth_commands::config_dir().await.join("credentials.enc")
}

/// Saves credentials JSON to an encrypted file.
pub async fn save_encrypted(json: &str) -> anyhow::Result<PathBuf> {
    let path = encrypted_credentials_path().await;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(parent).await?.permissions();
            perms.set_mode(0o700);
            if let Err(e) = tokio::fs::set_permissions(parent, perms).await
            {
                eprintln!(
                    "Warning: failed to set directory permissions on {}: {e}",
                    parent.display()
                );
            }
        }
    }

    let encrypted = encrypt(json.as_bytes()).await?;

    // Write atomically via a sibling .tmp file + rename so the credentials
    // file is never left in a corrupt partial-write state on crash/Ctrl-C.
    crate::fs_util::atomic_write_async(&path, &encrypted).await
        .map_err(|e| anyhow::anyhow!("Failed to write credentials: {e}"))?;

    // Set permissions to 600 on Unix (contains secrets)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&path).await?.permissions();
        perms.set_mode(0o600);
        if let Err(e) = tokio::fs::set_permissions(&path, perms).await {
            eprintln!(
                "Warning: failed to set file permissions on {}: {e}",
                path.display()
            );
        }
    }

    Ok(path)
}

/// Loads and decrypts credentials JSON from a specific path.
pub async fn load_encrypted_from_path(path: &std::path::Path) -> anyhow::Result<String> {
    let data = tokio::fs::read(path).await?;
    let plaintext = decrypt(&data).await?;
    Ok(String::from_utf8(plaintext)?)
}

/// Loads and decrypts credentials JSON from the default encrypted file.
pub async fn load_encrypted() -> anyhow::Result<String> {
    let path = encrypted_credentials_path().await;
    load_encrypted_from_path(&path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_or_create_key_is_deterministic() {
        let key1 = get_or_create_key().await.unwrap();
        let key2 = get_or_create_key().await.unwrap();
        assert_eq!(key1, key2);
    }

    #[tokio::test]
    async fn get_or_create_key_produces_256_bits() {
        let key = get_or_create_key().await.unwrap();
        assert_eq!(key.len(), 32);
    }

    #[tokio::test]
    async fn encrypt_decrypt_round_trip() {
        let plaintext = b"hello, world!";
        let encrypted = encrypt(plaintext).await.expect("encryption should succeed");
        assert_ne!(&encrypted, plaintext);
        assert_eq!(encrypted.len(), 12 + plaintext.len() + 16);
        let decrypted = decrypt(&encrypted).await.expect("decryption should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[tokio::test]
    async fn encrypt_decrypt_empty() {
        let plaintext = b"";
        let encrypted = encrypt(plaintext).await.expect("encryption should succeed");
        let decrypted = decrypt(&encrypted).await.expect("decryption should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[tokio::test]
    async fn decrypt_rejects_short_data() {
        let result = decrypt(&[0u8; 11]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[tokio::test]
    async fn decrypt_rejects_tampered_ciphertext() {
        let encrypted = encrypt(b"secret data").await.expect("encryption should succeed");
        let mut tampered = encrypted.clone();
        if tampered.len() > 12 {
            tampered[12] ^= 0xFF;
        }
        let result = decrypt(&tampered).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn each_encryption_produces_different_output() {
        let plaintext = b"same input";
        let enc1 = encrypt(plaintext).await.expect("encryption should succeed");
        let enc2 = encrypt(plaintext).await.expect("encryption should succeed");
        assert_ne!(enc1, enc2);
        let dec1 = decrypt(&enc1).await.unwrap();
        let dec2 = decrypt(&enc2).await.unwrap();
        assert_eq!(dec1, dec2);
        assert_eq!(dec1, plaintext);
    }
}

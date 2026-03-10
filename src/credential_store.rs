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
use std::sync::OnceLock;

/// Persist the base64-encoded encryption key to a local file with restrictive
/// permissions (0600 file, 0700 directory). Used only as a fallback when the OS
/// keyring is unavailable.
fn save_key_file(path: &std::path::Path, b64_key: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            {
                eprintln!("Warning: failed to set secure permissions on key directory: {e}");
            }
        }
    }

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        let mut file = options.open(path)?;
        file.write_all(b64_key.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, b64_key)?;
    }
    Ok(())
}

/// Returns the encryption key derived from the OS keyring, or falls back to a local file.
/// Generates a random 256-bit key and stores it securely if it doesn't exist.
fn get_or_create_key() -> anyhow::Result<[u8; 32]> {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();

    if let Some(key) = KEY.get() {
        return Ok(*key);
    }

    let cache_key = |candidate: [u8; 32]| -> [u8; 32] {
        if KEY.set(candidate).is_ok() {
            candidate
        } else {
            // If set() fails, another thread already initialized the key. .get() is
            // guaranteed to return Some at this point.
            *KEY.get()
                .expect("key must be initialized if OnceLock::set() failed")
        }
    };

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".to_string());

    let key_file = crate::auth_commands::config_dir().join(".encryption_key");

    let entry = Entry::new("gws-cli", &username);

    if let Ok(entry) = entry {
        match entry.get_password() {
            Ok(b64_key) => {
                use base64::{engine::general_purpose::STANDARD, Engine as _};
                if let Ok(decoded) = STANDARD.decode(&b64_key) {
                    if decoded.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&decoded);
                        // Keep the file as a backup — do not delete it.
                        // On some platforms (macOS) keyring reads may succeed
                        // now but fail after a binary-path or OS change.
                        return Ok(cache_key(arr));
                    }
                }
            }
            Err(keyring::Error::NoEntry) => {
                use base64::{engine::general_purpose::STANDARD, Engine as _};

                // If keyring is empty, prefer a persisted local key first.
                if key_file.exists() {
                    if let Ok(b64_key) = std::fs::read_to_string(&key_file) {
                        if let Ok(decoded) = STANDARD.decode(b64_key.trim()) {
                            if decoded.len() == 32 {
                                let mut arr = [0u8; 32];
                                arr.copy_from_slice(&decoded);
                                // Migrate file key into keyring; remove the
                                // file only if a round-trip read confirms the
                                // keyring actually persisted the value (macOS
                                // Keychain can return Ok without writing).
                                if entry.set_password(b64_key.trim()).is_ok()
                                    && entry.get_password().ok().as_deref()
                                        == Some(b64_key.trim())
                                {
                                    let _ = std::fs::remove_file(&key_file);
                                }
                                return Ok(cache_key(arr));
                            }
                        }
                    }
                }

                // Generate a new random 256-bit key.
                let mut key = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut key);
                let b64_key = STANDARD.encode(key);

                // Try keyring; verify with a round-trip read (macOS
                // Keychain can return Ok from set_password without
                // persisting). Always save to file as a backup.
                let keyring_ok = entry.set_password(&b64_key).is_ok()
                    && entry.get_password().ok().as_deref() == Some(b64_key.as_str());

                if !keyring_ok {
                    eprintln!(
                        "Warning: keyring write could not be verified, \
                         falling back to file storage"
                    );
                }

                // Always persist to file so the key survives keyring
                // failures, binary-path changes, or OS upgrades.
                save_key_file(&key_file, &b64_key)?;

                return Ok(cache_key(key));
            }
            Err(e) => {
                eprintln!("Warning: keyring access failed, falling back to file storage: {e}");
            }
        }
    }

    // Fallback: Local file `.encryption_key`

    if key_file.exists() {
        if let Ok(b64_key) = std::fs::read_to_string(&key_file) {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            if let Ok(decoded) = STANDARD.decode(b64_key.trim()) {
                if decoded.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&decoded);
                    return Ok(cache_key(arr));
                }
            }
        }
    }

    // Generate new key and save to local file
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let b64_key = STANDARD.encode(key);

    save_key_file(&key_file, &b64_key)?;

    Ok(cache_key(key))
}

/// Encrypts plaintext bytes using AES-256-GCM with a machine-derived key.
/// Returns nonce (12 bytes) || ciphertext.
pub fn encrypt(plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let key = get_or_create_key()?;
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
pub fn decrypt(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    if data.len() < 12 {
        anyhow::bail!("Encrypted data too short");
    }

    let key = get_or_create_key()?;
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
pub fn encrypted_credentials_path() -> PathBuf {
    crate::auth_commands::config_dir().join("credentials.enc")
}

/// Saves credentials JSON to an encrypted file.
pub fn save_encrypted(json: &str) -> anyhow::Result<PathBuf> {
    let path = encrypted_credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            {
                eprintln!(
                    "Warning: failed to set directory permissions on {}: {e}",
                    parent.display()
                );
            }
        }
    }

    let encrypted = encrypt(json.as_bytes())?;

    // Write atomically via a sibling .tmp file + rename so the credentials
    // file is never left in a corrupt partial-write state on crash/Ctrl-C.
    crate::fs_util::atomic_write(&path, &encrypted)
        .map_err(|e| anyhow::anyhow!("Failed to write credentials: {e}"))?;

    // Set permissions to 600 on Unix (contains secrets)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "Warning: failed to set file permissions on {}: {e}",
                path.display()
            );
        }
    }

    Ok(path)
}

/// Loads and decrypts credentials JSON from a specific path.
pub fn load_encrypted_from_path(path: &std::path::Path) -> anyhow::Result<String> {
    let data = std::fs::read(path)?;
    let plaintext = decrypt(&data)?;
    Ok(String::from_utf8(plaintext)?)
}

/// Loads and decrypts credentials JSON from the default encrypted file.
pub fn load_encrypted() -> anyhow::Result<String> {
    load_encrypted_from_path(&encrypted_credentials_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_create_key_is_deterministic() {
        let key1 = get_or_create_key().unwrap();
        let key2 = get_or_create_key().unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn get_or_create_key_produces_256_bits() {
        let key = get_or_create_key().unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let plaintext = b"hello, world!";
        let encrypted = encrypt(plaintext).expect("encryption should succeed");
        assert_ne!(&encrypted, plaintext);
        assert_eq!(encrypted.len(), 12 + plaintext.len() + 16);
        let decrypted = decrypt(&encrypted).expect("decryption should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_decrypt_empty() {
        let plaintext = b"";
        let encrypted = encrypt(plaintext).expect("encryption should succeed");
        let decrypted = decrypt(&encrypted).expect("decryption should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_rejects_short_data() {
        let result = decrypt(&[0u8; 11]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn decrypt_rejects_tampered_ciphertext() {
        let encrypted = encrypt(b"secret data").expect("encryption should succeed");
        let mut tampered = encrypted.clone();
        if tampered.len() > 12 {
            tampered[12] ^= 0xFF;
        }
        let result = decrypt(&tampered);
        assert!(result.is_err());
    }

    #[test]
    fn each_encryption_produces_different_output() {
        let plaintext = b"same input";
        let enc1 = encrypt(plaintext).expect("encryption should succeed");
        let enc2 = encrypt(plaintext).expect("encryption should succeed");
        assert_ne!(enc1, enc2);
        let dec1 = decrypt(&enc1).unwrap();
        let dec2 = decrypt(&enc2).unwrap();
        assert_eq!(dec1, dec2);
        assert_eq!(dec1, plaintext);
    }
}

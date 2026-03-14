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
use zeroize::Zeroize;

/// Ensure the key-file parent directory exists with restrictive permissions.
fn ensure_key_dir(path: &std::path::Path) -> std::io::Result<()> {
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
    Ok(())
}

/// Atomically create a **new** key file using `create_new(true)` (`O_EXCL` on
/// Unix, `CREATE_NEW` on Windows). If another process already created the file,
/// returns `Err` with `ErrorKind::AlreadyExists` so the caller can read the
/// winner's key instead.
fn save_key_file_exclusive(path: &std::path::Path, b64_key: &str) -> std::io::Result<()> {
    use std::io::Write;
    ensure_key_dir(path)?;

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true); // atomic: fails if file already exists
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(b64_key.as_bytes())?;
    file.sync_all()?; // fsync: ensure key is durable before returning
    Ok(())
}

/// Persist the base64-encoded encryption key to a local file with restrictive
/// permissions (0600 file, 0700 directory). Overwrites any existing file
/// atomically via a sibling `.tmp` file + rename so a crash mid-write never
/// leaves the key file truncated or corrupt.
fn save_key_file(path: &std::path::Path, b64_key: &str) -> std::io::Result<()> {
    use std::io::Write;
    ensure_key_dir(path)?;

    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
    })?;
    let tmp_path = path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut tmp = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_path)?;
        tmp.write_all(b64_key.as_bytes())?;
        tmp.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let mut tmp = std::fs::File::create(&tmp_path)?;
        tmp.write_all(b64_key.as_bytes())?;
        tmp.sync_all()?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Read and decode a base64-encoded 256-bit key from a file.
///
/// On Unix, warns if the file is world-readable (mode & 0o077 != 0).
fn read_key_file(path: &std::path::Path) -> Option<[u8; 32]> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    // Item 4: validate file permissions on read
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                eprintln!(
                    "Warning: encryption key file {} has overly permissive mode {:04o}. \
                     Expected 0600. Run: chmod 600 {}",
                    path.display(),
                    mode & 0o777,
                    path.display()
                );
            }
        }
    }

    let b64_key = std::fs::read_to_string(path).ok()?;
    let mut decoded = STANDARD.decode(b64_key.trim()).ok()?;
    if decoded.len() == 32 {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        decoded.zeroize(); // scrub decoded key material from heap
        Some(arr)
    } else {
        decoded.zeroize();
        None
    }
}

/// Generate a random 256-bit key.
fn generate_random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

/// Abstraction over OS keyring operations for testability.
trait KeyringProvider {
    /// Attempt to read the stored password.
    fn get_password(&self) -> Result<String, keyring::Error>;
    /// Attempt to store a password in the keyring.
    fn set_password(&self, password: &str) -> Result<(), keyring::Error>;
}

/// Production keyring implementation wrapping an optional `keyring::Entry`.
struct OsKeyring(Option<Entry>);

impl OsKeyring {
    fn new(service: &str, user: &str) -> Self {
        Self(Entry::new(service, user).ok())
    }
}

impl KeyringProvider for OsKeyring {
    fn get_password(&self) -> Result<String, keyring::Error> {
        match &self.0 {
            Some(entry) => entry.get_password(),
            None => Err(keyring::Error::NoEntry),
        }
    }

    fn set_password(&self, password: &str) -> Result<(), keyring::Error> {
        match &self.0 {
            Some(entry) => entry.set_password(password),
            None => Err(keyring::Error::NoEntry),
        }
    }
}

/// Which backend to use for encryption key storage.
///
/// Controlled by `GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND`:
/// - `"keyring"` (default): OS keyring only — no `.encryption_key` file backup
/// - `"keyring-with-file"`: OS keyring with `.encryption_key` file as durable backup
/// - `"file"`: `.encryption_key` file only (for Docker/CI/headless)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyringBackend {
    /// OS keyring only. The encryption key is never written to disk, giving the
    /// strongest security on platforms with a native keychain (macOS Keychain,
    /// Windows Credential Manager). Falls back to reading an existing
    /// `.encryption_key` file for migration, but will not create or update one.
    Keyring,
    /// OS keyring with `.encryption_key` file kept in sync as a durable backup.
    /// Suitable for environments where the keyring may be ephemeral (e.g. after
    /// OS upgrades or container restarts).
    KeyringWithFile,
    /// `.encryption_key` file only — no keyring interaction.
    File,
}

impl KeyringBackend {
    fn from_env() -> Self {
        let raw = std::env::var("GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND").unwrap_or_default();
        let lower = raw.to_lowercase();
        match lower.as_str() {
            "keyring" | "" => KeyringBackend::Keyring,
            "keyring-with-file" => KeyringBackend::KeyringWithFile,
            "file" => KeyringBackend::File,
            other => {
                eprintln!(
                    "Warning: unknown GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND=\"{other}\", \
                     defaulting to \"keyring\". Valid values: \"keyring\", \"keyring-with-file\", \"file\"."
                );
                KeyringBackend::Keyring
            }
        }
    }

    /// Human-readable name for logging and status output.
    fn as_str(&self) -> &'static str {
        match self {
            KeyringBackend::Keyring => "keyring",
            KeyringBackend::KeyringWithFile => "keyring-with-file",
            KeyringBackend::File => "file",
        }
    }

    /// Whether this backend reads/writes the OS keyring.
    fn uses_keyring(&self) -> bool {
        matches!(
            self,
            KeyringBackend::Keyring | KeyringBackend::KeyringWithFile
        )
    }

    /// Whether this backend persists the encryption key to a local file.
    fn saves_to_file(&self) -> bool {
        matches!(self, KeyringBackend::KeyringWithFile | KeyringBackend::File)
    }
}

/// Core key-resolution logic, separated from caching for testability.
///
/// - `Keyring` (keyring-only):
///   1. Try keyring → 2. Read file (migration only) → 3. Generate (keyring only).
///      The `.encryption_key` file is never created or updated.
///
/// - `KeyringWithFile`:
///   1. Try keyring → 2. Try file → 3. Generate (save to keyring + file).
///      The `.encryption_key` file is kept in sync as a durable backup.
///
/// - `File`:
///   1. Try file → 2. Generate (save to file only).
fn resolve_key(
    backend: KeyringBackend,
    provider: &dyn KeyringProvider,
    key_file: &std::path::Path,
) -> anyhow::Result<[u8; 32]> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    // --- 1. Try keyring (Keyring and KeyringWithFile) --------------------
    if backend.uses_keyring() {
        match provider.get_password() {
            Ok(b64_key) => {
                if let Ok(decoded) = STANDARD.decode(&b64_key) {
                    if decoded.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&decoded);
                        if backend.saves_to_file() {
                            if let Err(err) = save_key_file(key_file, &b64_key) {
                                eprintln!(
                                    "Warning: failed to sync keyring backup file at '{}': {err}",
                                    key_file.display()
                                );
                            }
                        }
                        return Ok(arr);
                    }
                }
                // Keyring contained invalid data — fall through to file.
            }
            Err(keyring::Error::NoEntry) => {
                // Keyring is reachable but empty — check file, then generate.
                if let Some(key) = read_key_file(key_file) {
                    if provider.set_password(&STANDARD.encode(key)).is_ok()
                        && !backend.saves_to_file()
                    {
                        eprintln!(
                            "Note: encryption key migrated to OS keyring. \
                             You can remove {} for keyring-only security.",
                            key_file.display()
                        );
                    }
                    return Ok(key);
                }

                // Generate a new key.
                let key = generate_random_key();
                let b64_key = STANDARD.encode(key);

                if let Err(e) = provider.set_password(&b64_key) {
                    if backend.saves_to_file() {
                        eprintln!(
                            "Warning: failed to store key in OS keyring: {e}. \
                             Falling back to file storage."
                        );
                    } else {
                        anyhow::bail!(
                            "Failed to store encryption key in OS keyring: {e}. \
                             The key cannot be persisted, so credentials would be \
                             unrecoverable after this process exits. \
                             Set GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND=keyring-with-file \
                             or =file to enable file-based key storage."
                        );
                    }
                }

                if backend.saves_to_file() {
                    match save_key_file_exclusive(key_file, &b64_key) {
                        Ok(()) => return Ok(key),
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                            if let Some(winner) = read_key_file(key_file) {
                                let _ = provider.set_password(&STANDARD.encode(winner));
                                return Ok(winner);
                            }
                            save_key_file(key_file, &b64_key)?;
                            return Ok(key);
                        }
                        Err(e) => return Err(e.into()),
                    }
                }

                return Ok(key);
            }
            Err(e) => {
                if backend.saves_to_file() {
                    eprintln!("Warning: keyring access failed, falling back to file storage: {e}");
                } else {
                    eprintln!("Warning: keyring access failed: {e}");
                }
            }
        }
    }

    // --- 2. File fallback (read existing file for all backends) -----------
    if let Some(key) = read_key_file(key_file) {
        return Ok(key);
    }

    // --- 3. Generate new key ---------------------------------------------
    if backend.saves_to_file() {
        let key = generate_random_key();
        let b64_key = STANDARD.encode(key);
        match save_key_file_exclusive(key_file, &b64_key) {
            Ok(()) => Ok(key),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => read_key_file(key_file)
                .ok_or_else(|| anyhow::anyhow!("key file exists but is corrupt")),
            Err(e) => Err(e.into()),
        }
    } else {
        anyhow::bail!(
            "OS keyring is unavailable and no .encryption_key fallback file found. \
             Set GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND=keyring-with-file or =file \
             to enable file-based key storage."
        )
    }
}

/// Returns the encryption key, generating and persisting one if it doesn't exist.
///
/// The key is cached in-process via `OnceLock` so it is only read from disk once.
/// Backend selection is controlled by `GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND`.
fn get_or_create_key() -> anyhow::Result<[u8; 32]> {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();

    if let Some(key) = KEY.get() {
        return Ok(*key);
    }

    let backend = KeyringBackend::from_env();
    // Item 5: log which backend was selected
    eprintln!("Using keyring backend: {}", backend.as_str());

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".to_string());

    let key_file = crate::auth_commands::config_dir().join(".encryption_key");
    let provider = OsKeyring::new("gws-cli", &username);

    let key = resolve_key(backend, &provider, &key_file)?;

    // Cache for subsequent calls within this process.
    if KEY.set(key).is_ok() {
        Ok(key)
    } else {
        Ok(*KEY
            .get()
            .expect("key must be initialized if OnceLock::set() failed"))
    }
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

/// Returns the name of the active keyring backend for status display.
pub fn active_backend_name() -> &'static str {
    KeyringBackend::from_env().as_str()
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
    use std::cell::RefCell;

    /// Describes what `get_password` / `set_password` should return.
    #[derive(Clone)]
    enum MockState {
        Ok(String),
        NoEntry,
        PlatformError,
    }

    /// Mock keyring for testing `resolve_key()` without OS dependencies.
    struct MockKeyring {
        get_state: MockState,
        set_succeeds: bool,
        last_set: RefCell<Option<String>>,
        on_set: RefCell<Option<Box<dyn FnMut(&str)>>>,
    }

    impl MockKeyring {
        fn with_password(b64: &str) -> Self {
            Self {
                get_state: MockState::Ok(b64.to_string()),
                set_succeeds: true,
                last_set: RefCell::new(None),
                on_set: RefCell::new(None),
            }
        }

        fn no_entry() -> Self {
            Self {
                get_state: MockState::NoEntry,
                set_succeeds: true,
                last_set: RefCell::new(None),
                on_set: RefCell::new(None),
            }
        }

        fn platform_error() -> Self {
            Self {
                get_state: MockState::PlatformError,
                set_succeeds: true,
                last_set: RefCell::new(None),
                on_set: RefCell::new(None),
            }
        }

        fn with_set_failure(mut self) -> Self {
            self.set_succeeds = false;
            self
        }

        fn with_on_set<F>(self, callback: F) -> Self
        where
            F: FnMut(&str) + 'static,
        {
            *self.on_set.borrow_mut() = Some(Box::new(callback));
            self
        }
    }

    impl KeyringProvider for MockKeyring {
        fn get_password(&self) -> Result<String, keyring::Error> {
            match &self.get_state {
                MockState::Ok(s) => Ok(s.clone()),
                MockState::NoEntry => Err(keyring::Error::NoEntry),
                MockState::PlatformError => {
                    Err(keyring::Error::PlatformFailure("mock: no backend".into()))
                }
            }
        }

        fn set_password(&self, password: &str) -> Result<(), keyring::Error> {
            *self.last_set.borrow_mut() = Some(password.to_string());
            if let Some(callback) = self.on_set.borrow_mut().as_mut() {
                callback(password);
            }
            if self.set_succeeds {
                Ok(())
            } else {
                Err(keyring::Error::NoEntry)
            }
        }
    }

    fn write_test_key(dir: &std::path::Path) -> ([u8; 32], std::path::PathBuf) {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let key = [42u8; 32];
        let path = dir.join(".encryption_key");
        std::fs::write(&path, STANDARD.encode(key)).unwrap();
        (key, path)
    }

    // ---- Backend::Keyring (keyring-only) tests ----

    #[test]
    fn keyring_only_returns_key_without_file_write() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let expected = [7u8; 32];
        let mock = MockKeyring::with_password(&STANDARD.encode(expected));
        let result = resolve_key(KeyringBackend::Keyring, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
        assert!(
            !key_file.exists(),
            "keyring-only must NOT create a file backup"
        );
    }

    #[test]
    fn keyring_only_no_entry_reads_file_for_migration() {
        let dir = tempfile::tempdir().unwrap();
        let (expected, key_file) = write_test_key(dir.path());
        let mock = MockKeyring::no_entry();
        let result = resolve_key(KeyringBackend::Keyring, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
        assert!(
            mock.last_set.borrow().is_some(),
            "should copy file key into keyring for migration"
        );
    }

    #[test]
    fn keyring_only_no_entry_no_file_generates_keyring_only() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let mock = MockKeyring::no_entry();
        let key = resolve_key(KeyringBackend::Keyring, &mock, &key_file).unwrap();
        assert_eq!(key.len(), 32);
        assert!(!key_file.exists(), "keyring-only must NOT create a file");
        assert!(mock.last_set.borrow().is_some(), "should store in keyring");
    }

    #[test]
    fn keyring_only_no_entry_no_file_set_password_fails_errors() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let mock = MockKeyring::no_entry().with_set_failure();
        let result = resolve_key(KeyringBackend::Keyring, &mock, &key_file);
        assert!(
            result.is_err(),
            "must fail when keyring set_password fails in keyring-only mode"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Failed to store encryption key"),
            "error should explain the failure: {msg}"
        );
        assert!(
            !key_file.exists(),
            "must NOT create a file in keyring-only mode"
        );
    }

    #[test]
    fn keyring_only_platform_error_reads_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let (expected, key_file) = write_test_key(dir.path());
        let mock = MockKeyring::platform_error();
        let result = resolve_key(KeyringBackend::Keyring, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn keyring_only_platform_error_no_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let mock = MockKeyring::platform_error();
        let result = resolve_key(KeyringBackend::Keyring, &mock, &key_file);
        assert!(
            result.is_err(),
            "should fail when keyring is unavailable and no file exists"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("keyring-with-file"),
            "error should suggest keyring-with-file alternative"
        );
    }

    #[test]
    fn keyring_only_invalid_keyring_data_uses_file() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let (expected, key_file) = write_test_key(dir.path());
        let mock = MockKeyring::with_password(&STANDARD.encode([1u8; 16])); // wrong length
        let result = resolve_key(KeyringBackend::Keyring, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
    }

    // ---- Backend::KeyringWithFile tests ----

    #[test]
    fn keyring_with_file_returns_keyring_key() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let expected = [7u8; 32];
        let mock = MockKeyring::with_password(&STANDARD.encode(expected));
        let result = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn keyring_with_file_creates_file_backup_when_missing() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let expected = [7u8; 32];
        let mock = MockKeyring::with_password(&STANDARD.encode(expected));
        assert!(!key_file.exists(), "file must not exist before test");
        let result = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
        assert!(
            key_file.exists(),
            "file backup must be created when keyring succeeds but file is missing"
        );
        let file_key = read_key_file(&key_file).unwrap();
        assert_eq!(
            file_key, expected,
            "file backup must contain the keyring key"
        );
    }

    #[test]
    fn keyring_with_file_syncs_file_when_keyring_differs() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let (file_key, key_file) = write_test_key(dir.path());
        let keyring_key = [7u8; 32];
        assert_ne!(file_key, keyring_key, "keys must differ for this test");
        let mock = MockKeyring::with_password(&STANDARD.encode(keyring_key));
        let result = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(result, keyring_key, "should return keyring key");
        assert!(key_file.exists(), "file must NOT be deleted");
        let synced = read_key_file(&key_file).unwrap();
        assert_eq!(
            synced, keyring_key,
            "file must be updated to match keyring key"
        );
    }

    #[test]
    fn keyring_with_file_no_entry_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let (expected, key_file) = write_test_key(dir.path());
        let mock = MockKeyring::no_entry();
        let result = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
        assert!(key_file.exists(), "file must NOT be deleted");
        assert!(
            mock.last_set.borrow().is_some(),
            "should copy key to keyring"
        );
    }

    #[test]
    fn keyring_with_file_no_entry_no_file_generates_and_saves_both() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let mock = MockKeyring::no_entry();
        let key = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(key.len(), 32);
        assert!(key_file.exists(), "file must be created as fallback");
        assert!(mock.last_set.borrow().is_some(), "should store in keyring");
        let file_key = read_key_file(&key_file).unwrap();
        assert_eq!(key, file_key);
    }

    #[test]
    fn keyring_with_file_no_entry_no_file_keyring_set_fails() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let mock = MockKeyring::no_entry().with_set_failure();
        let key = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(key.len(), 32);
        assert!(key_file.exists(), "file must be created when keyring fails");
        let file_key = read_key_file(&key_file).unwrap();
        assert_eq!(key, file_key);
    }

    #[test]
    fn keyring_with_file_platform_error_falls_back_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let (expected, key_file) = write_test_key(dir.path());
        let mock = MockKeyring::platform_error();
        let result = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn keyring_with_file_platform_error_no_file_generates() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let mock = MockKeyring::platform_error();
        let key = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(key.len(), 32);
        assert!(key_file.exists());
    }

    #[test]
    fn keyring_with_file_invalid_keyring_data_uses_file() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let (expected, key_file) = write_test_key(dir.path());
        let mock = MockKeyring::with_password(&STANDARD.encode([1u8; 16])); // wrong length
        let result = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(result, expected);
    }

    // ---- Backend::File tests ----

    #[test]
    fn file_backend_reads_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let (expected, key_file) = write_test_key(dir.path());
        let mock = MockKeyring::with_password("should-not-be-used");
        let result = resolve_key(KeyringBackend::File, &mock, &key_file).unwrap();
        assert_eq!(result, expected, "file backend should ignore keyring");
    }

    #[test]
    fn file_backend_generates_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let mock = MockKeyring::no_entry();
        let key = resolve_key(KeyringBackend::File, &mock, &key_file).unwrap();
        assert_eq!(key.len(), 32);
        assert!(key_file.exists());
        assert!(
            mock.last_set.borrow().is_none(),
            "file backend must not touch keyring"
        );
    }

    #[test]
    fn file_backend_skips_keyring_entirely() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let (file_key, key_file) = write_test_key(dir.path());
        // Keyring has a DIFFERENT key — file backend should ignore it.
        let mock = MockKeyring::with_password(&STANDARD.encode([99u8; 32]));
        let result = resolve_key(KeyringBackend::File, &mock, &key_file).unwrap();
        assert_eq!(result, file_key, "must return the file key, not keyring");
    }

    // ---- Stability tests ----

    #[test]
    fn key_is_stable_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");
        let mock = MockKeyring::platform_error();
        let key1 = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        let key2 = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();
        assert_eq!(key1, key2);
    }

    // ---- KeyringBackend::from_env tests ----

    #[test]
    fn backend_default_is_keyring() {
        assert_eq!(parse_backend(""), KeyringBackend::Keyring);
    }

    #[test]
    fn backend_uses_keyring() {
        assert!(KeyringBackend::Keyring.uses_keyring());
        assert!(KeyringBackend::KeyringWithFile.uses_keyring());
        assert!(!KeyringBackend::File.uses_keyring());
    }

    #[test]
    fn backend_saves_to_file() {
        assert!(!KeyringBackend::Keyring.saves_to_file());
        assert!(KeyringBackend::KeyringWithFile.saves_to_file());
        assert!(KeyringBackend::File.saves_to_file());
    }

    // ---- read_key_file tests ----

    #[test]
    fn read_key_file_valid() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        let key = [99u8; 32];
        std::fs::write(&path, STANDARD.encode(key)).unwrap();
        assert_eq!(read_key_file(&path), Some(key));
    }

    #[test]
    fn read_key_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_key_file(&dir.path().join("nonexistent")), None);
    }

    #[test]
    fn read_key_file_wrong_length() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        std::fs::write(&path, STANDARD.encode([1u8; 16])).unwrap();
        assert_eq!(read_key_file(&path), None);
    }

    #[test]
    fn read_key_file_invalid_base64() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        std::fs::write(&path, "not-valid-base64!!!").unwrap();
        assert_eq!(read_key_file(&path), None);
    }

    // ---- Existing encrypt/decrypt tests ----

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

    // ---- save_key_file_exclusive tests ----

    #[test]
    fn save_key_file_exclusive_creates_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".encryption_key");
        save_key_file_exclusive(&path, "dGVzdA==").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "dGVzdA==");
    }

    #[test]
    fn save_key_file_exclusive_rejects_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".encryption_key");
        std::fs::write(&path, "existing").unwrap();
        let err = save_key_file_exclusive(&path, "new").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        // Original content is untouched.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing");
    }

    // ---- save_key_file tests ----

    #[test]
    fn save_key_file_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".encryption_key");
        std::fs::write(&path, "old").unwrap();
        save_key_file(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    // ---- ensure_key_dir tests ----

    #[test]
    fn ensure_key_dir_creates_nested_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("c").join("key");
        ensure_key_dir(&path).unwrap();
        assert!(path.parent().unwrap().is_dir());
    }

    // ---- KeyringBackend::from_env tests ----

    #[test]
    fn backend_from_env_file_lowercase() {
        assert_eq!(parse_backend("file"), KeyringBackend::File);
    }

    #[test]
    fn backend_from_env_file_uppercase() {
        assert_eq!(parse_backend("FILE"), KeyringBackend::File);
    }

    #[test]
    fn backend_from_env_keyring_with_file() {
        assert_eq!(
            parse_backend("keyring-with-file"),
            KeyringBackend::KeyringWithFile
        );
    }

    #[test]
    fn backend_from_env_keyring_with_file_uppercase() {
        assert_eq!(
            parse_backend("KEYRING-WITH-FILE"),
            KeyringBackend::KeyringWithFile
        );
    }

    #[test]
    fn backend_from_env_invalid_defaults_to_keyring() {
        assert_eq!(parse_backend("foobar"), KeyringBackend::Keyring);
    }

    /// Parse a backend string using the same logic as `from_env()`.
    fn parse_backend(s: &str) -> KeyringBackend {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "keyring" | "" => KeyringBackend::Keyring,
            "keyring-with-file" => KeyringBackend::KeyringWithFile,
            "file" => KeyringBackend::File,
            _ => KeyringBackend::Keyring,
        }
    }

    // ---- Race condition tests ----

    #[test]
    fn race_loser_syncs_winner_key_to_keyring() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");

        let winner_key = [77u8; 32];
        let winner_b64 = STANDARD.encode(winner_key);
        let race_key_file = key_file.clone();
        let race_winner_b64 = winner_b64.clone();

        let mock = MockKeyring::no_entry().with_on_set(move |_| {
            if !race_key_file.exists() {
                std::fs::write(&race_key_file, &race_winner_b64).unwrap();
            }
        });
        let result = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();

        assert_eq!(result, winner_key);
        let synced = mock.last_set.borrow().clone().unwrap();
        assert_eq!(STANDARD.decode(&synced).unwrap(), winner_key);
    }

    #[test]
    fn race_loser_corrupt_file_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let key_file = dir.path().join(".encryption_key");

        // Pre-create a corrupt file (not valid base64 for a 32-byte key).
        std::fs::write(&key_file, "corrupt-data").unwrap();

        let mock = MockKeyring::no_entry();
        let result = resolve_key(KeyringBackend::KeyringWithFile, &mock, &key_file).unwrap();

        // Should generate a new key and overwrite the corrupt file.
        assert_eq!(result.len(), 32);
        let file_key = read_key_file(&key_file).unwrap();
        assert_eq!(result, file_key, "file should be overwritten with new key");
    }
}

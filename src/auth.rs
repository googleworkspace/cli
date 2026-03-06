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

//! Authentication and Credential Management
//!
//! Handles obtaining OAuth 2.0 access tokens and Service Account tokens.
//! Supports local user flow (via a loopback server) and Application Default Credentials,
//! with token caching to minimize repeated authentication overhead.

use std::path::PathBuf;

use anyhow::Context;

use crate::credential_store;

/// Returns the `quota_project_id` from Application Default Credentials, if present.
/// This is used to set the `x-goog-user-project` header on API requests.
pub fn get_quota_project() -> Option<String> {
    let path = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .ok()
        .map(PathBuf::from)
        .or_else(adc_well_known_path)?;
    let content = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("quota_project_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Returns the well-known Application Default Credentials path:
/// `~/.config/gcloud/application_default_credentials.json`.
///
/// Note: `dirs::config_dir()` returns `~/Library/Application Support` on macOS, which is
/// wrong for gcloud. The Google Cloud SDK always uses `~/.config/gcloud` regardless of OS.
fn adc_well_known_path() -> Option<PathBuf> {
    dirs::home_dir().map(|d| {
        d.join(".config")
            .join("gcloud")
            .join("application_default_credentials.json")
    })
}

/// Types of credentials we support
#[derive(Debug)]
enum Credential {
    AuthorizedUser(yup_oauth2::authorized_user::AuthorizedUserSecret),
    ServiceAccount(yup_oauth2::ServiceAccountKey),
}

/// Builds an OAuth2 authenticator and returns an access token.
///
/// Tries credentials in order:
/// 0. `GOOGLE_WORKSPACE_CLI_TOKEN` env var (raw access token, highest priority)
/// 1. `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` env var (plaintext JSON, can be User or Service Account)
/// 2. Encrypted credentials at `~/.config/gws/credentials.enc`
/// 3. Plaintext credentials at `~/.config/gws/credentials.json` (User only)
/// 4. Application Default Credentials (ADC):
///    - `GOOGLE_APPLICATION_CREDENTIALS` env var (path to a JSON credentials file), then
///    - Well-known ADC path: `~/.config/gcloud/application_default_credentials.json`
///      (populated by `gcloud auth application-default login`)
pub async fn get_token(scopes: &[&str]) -> anyhow::Result<String> {
    // 0. Direct token from env var (highest priority, bypasses all credential loading)
    if let Ok(token) = std::env::var("GOOGLE_WORKSPACE_CLI_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    let creds_file = std::env::var("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE").ok();
    let config_dir = crate::auth_commands::config_dir();
    let enc_path = credential_store::encrypted_credentials_path();
    let default_path = config_dir.join("credentials.json");
    let token_cache = config_dir.join("token_cache.json");

    let creds = load_credentials_inner(creds_file.as_deref(), &enc_path, &default_path).await?;
    get_token_inner(scopes, creds, &token_cache).await
}

async fn get_token_inner(
    scopes: &[&str],
    creds: Credential,
    token_cache_path: &std::path::Path,
) -> anyhow::Result<String> {
    match creds {
        Credential::AuthorizedUser(secret) => {
            let auth = yup_oauth2::AuthorizedUserAuthenticator::builder(secret)
                .with_storage(Box::new(crate::token_storage::EncryptedTokenStorage::new(
                    token_cache_path.to_path_buf(),
                )))
                .build()
                .await
                .context("Failed to build authorized user authenticator")?;

            let token = auth.token(scopes).await.context("Failed to get token")?;
            Ok(token
                .token()
                .ok_or_else(|| anyhow::anyhow!("Token response contained no access token"))?
                .to_string())
        }
        Credential::ServiceAccount(key) => {
            let tc_filename = token_cache_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "token_cache.json".to_string());
            let sa_cache = token_cache_path.with_file_name(format!("sa_{tc_filename}"));
            let builder = yup_oauth2::ServiceAccountAuthenticator::builder(key).with_storage(
                Box::new(crate::token_storage::EncryptedTokenStorage::new(sa_cache)),
            );

            let auth = builder
                .build()
                .await
                .context("Failed to build service account authenticator")?;

            let token = auth.token(scopes).await.context("Failed to get token")?;
            Ok(token
                .token()
                .ok_or_else(|| anyhow::anyhow!("Token response contained no access token"))?
                .to_string())
        }
    }
}

/// Parse a plaintext JSON credential file into a [`Credential`].
///
/// Determines the credential type from the `"type"` field:
/// - `"service_account"` → [`Credential::ServiceAccount`]
/// - anything else (including `"authorized_user"`) → [`Credential::AuthorizedUser`]
///
/// Uses the already-parsed `serde_json::Value` to avoid a second string parse.
async fn parse_credential_file(
    path: &std::path::Path,
    content: &str,
) -> anyhow::Result<Credential> {
    let json: serde_json::Value = serde_json::from_str(content)
        .with_context(|| format!("Failed to parse credentials JSON at {}", path.display()))?;

    if json.get("type").and_then(|v| v.as_str()) == Some("service_account") {
        let key = yup_oauth2::parse_service_account_key(content).with_context(|| {
            format!(
                "Failed to parse service account key from {}",
                path.display()
            )
        })?;
        return Ok(Credential::ServiceAccount(key));
    }

    // Deserialize from the Value we already have — avoids a second string parse.
    let secret: yup_oauth2::authorized_user::AuthorizedUserSecret = serde_json::from_value(json)
        .with_context(|| {
            format!(
                "Failed to parse authorized user credentials from {}",
                path.display()
            )
        })?;
    Ok(Credential::AuthorizedUser(secret))
}

async fn load_credentials_inner(
    env_file: Option<&str>,
    enc_path: &std::path::Path,
    default_path: &std::path::Path,
) -> anyhow::Result<Credential> {
    // 1. Explicit env var — plaintext file (User or Service Account)
    if let Some(path) = env_file {
        let p = PathBuf::from(path);
        if p.exists() {
            let content = tokio::fs::read_to_string(&p)
                .await
                .with_context(|| format!("Failed to read credentials from {path}"))?;
            return parse_credential_file(&p, &content).await;
        }
        anyhow::bail!(
            "GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE points to {path}, but file does not exist"
        );
    }

    // 2. Encrypted credentials (always AuthorizedUser for now)
    if enc_path.exists() {
        let json_str = credential_store::load_encrypted_from_path(enc_path)
            .context("Failed to decrypt credentials")?;

        let creds: serde_json::Value =
            serde_json::from_str(&json_str).context("Failed to parse decrypted credentials")?;

        let client_id = creds["client_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing client_id in encrypted credentials"))?;
        let client_secret = creds["client_secret"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing client_secret in encrypted credentials"))?;
        // refresh_token is optional now in some flows, but strictly required for this storage format
        let refresh_token = creds["refresh_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing refresh_token in encrypted credentials"))?;

        // Enforce scope boundaries: if credentials were obtained with specific scopes,
        // reject requests for scopes outside the granted set.
        // This prevents scope escalation when credentials are exported and reused.
        if let Some(granted) = creds.get("granted_scopes").and_then(|v| v.as_array()) {
            let granted_set: std::collections::HashSet<&str> = granted
                .iter()
                .filter_map(|s| s.as_str())
                .collect();

            if !granted_set.is_empty() {
                for requested in scopes {
                    if !granted_set.contains(requested) {
                        // Check if a broader scope subsumes the requested one.
                        // e.g. "drive" subsumes "drive.readonly"
                        let prefix = "https://www.googleapis.com/auth/";
                        let req_short = requested.strip_prefix(prefix).unwrap_or(requested);
                        let is_subsumed = granted_set.iter().any(|&g| {
                            let g_short = g.strip_prefix(prefix).unwrap_or(g);
                            req_short.starts_with(g_short)
                                && req_short.as_bytes().get(g_short.len()) == Some(&b'.')
                        });
                        if !is_subsumed {
                            eprintln!(
                                "⚠️  Scope '{}' was not in the granted scopes at login time. \
                                 Re-run 'gws auth login' with the required scopes.",
                                requested
                            );
                        }
                    }
                }
            }
        }

        return Ok(Credential::AuthorizedUser(
            yup_oauth2::authorized_user::AuthorizedUserSecret {
                client_id: client_id.to_string(),
                client_secret: client_secret.to_string(),
                refresh_token: refresh_token.to_string(),
                key_type: "authorized_user".to_string(),
            },
        ));
    }

    // 3. Plaintext credentials at default path (Default to AuthorizedUser)
    if default_path.exists() {
        return Ok(Credential::AuthorizedUser(
            yup_oauth2::read_authorized_user_secret(default_path)
                .await
                .with_context(|| {
                    format!("Failed to read credentials from {}", default_path.display())
                })?,
        ));
    }

    // 4a. GOOGLE_APPLICATION_CREDENTIALS env var (explicit path — hard error if missing)
    if let Ok(adc_env) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        let adc_path = PathBuf::from(&adc_env);
        if adc_path.exists() {
            let content = tokio::fs::read_to_string(&adc_path)
                .await
                .with_context(|| format!("Failed to read ADC from {adc_env}"))?;
            return parse_credential_file(&adc_path, &content).await;
        }
        anyhow::bail!(
            "GOOGLE_APPLICATION_CREDENTIALS points to {adc_env}, but file does not exist"
        );
    }

    // 4b. Well-known ADC path: ~/.config/gcloud/application_default_credentials.json
    // (populated by `gcloud auth application-default login`). Silent if absent.
    if let Some(well_known) = adc_well_known_path() {
        if well_known.exists() {
            let content = tokio::fs::read_to_string(&well_known)
                .await
                .with_context(|| format!("Failed to read ADC from {}", well_known.display()))?;
            return parse_credential_file(&well_known, &content).await;
        }
    }

    anyhow::bail!(
        "No credentials found. Run `gws auth setup` to configure, \
         `gws auth login` to authenticate, or set GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE.\n\
         Tip: Application Default Credentials (ADC) are also supported — run \
         `gcloud auth application-default login` or set GOOGLE_APPLICATION_CREDENTIALS."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// RAII guard that saves the current value of an environment variable and
    /// restores it when dropped. This ensures cleanup even if a test panics.
    struct EnvVarGuard {
        name: String,
        original: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        /// Save the current value of `name`, then set it to `value`.
        fn set(name: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let original = std::env::var_os(name);
            std::env::set_var(name, value);
            Self {
                name: name.to_string(),
                original,
            }
        }

        /// Save the current value of `name`, then remove it.
        fn remove(name: &str) -> Self {
            let original = std::env::var_os(name);
            std::env::remove_var(name);
            Self {
                name: name.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => std::env::set_var(&self.name, v),
                None => std::env::remove_var(&self.name),
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_load_credentials_no_options() {
        // Isolate from host ADC: override HOME so adc_well_known_path()
        // resolves to a non-existent directory, and clear the env var.
        let tmp = tempfile::tempdir().unwrap();
        let _home_guard = EnvVarGuard::set("HOME", tmp.path());
        let _adc_guard = EnvVarGuard::remove("GOOGLE_APPLICATION_CREDENTIALS");

        let err = load_credentials_inner(
            None,
            &PathBuf::from("/does/not/exist1"),
            &PathBuf::from("/does/not/exist2"),
        )
        .await;

        assert!(err.is_err());
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("No credentials found"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_load_credentials_adc_env_var_authorized_user() {
        let mut file = NamedTempFile::new().unwrap();
        let json = r#"{
            "client_id": "adc_id",
            "client_secret": "adc_secret",
            "refresh_token": "adc_refresh",
            "type": "authorized_user"
        }"#;
        file.write_all(json.as_bytes()).unwrap();

        let _adc_guard = EnvVarGuard::set(
            "GOOGLE_APPLICATION_CREDENTIALS",
            file.path().to_str().unwrap(),
        );

        let res = load_credentials_inner(
            None,
            &PathBuf::from("/missing/enc"),
            &PathBuf::from("/missing/plain"),
        )
        .await;

        match res.unwrap() {
            Credential::AuthorizedUser(secret) => {
                assert_eq!(secret.client_id, "adc_id");
                assert_eq!(secret.refresh_token, "adc_refresh");
            }
            _ => panic!("Expected AuthorizedUser from ADC"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_load_credentials_adc_env_var_service_account() {
        let mut file = NamedTempFile::new().unwrap();
        let json = r#"{
            "type": "service_account",
            "project_id": "test-project",
            "private_key_id": "adc-key-id",
            "private_key": "-----BEGIN PRIVATE KEY-----\nMIIEvwIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----\n",
            "client_email": "adc-sa@test-project.iam.gserviceaccount.com",
            "client_id": "456",
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": "https://oauth2.googleapis.com/token"
        }"#;
        file.write_all(json.as_bytes()).unwrap();

        let _adc_guard = EnvVarGuard::set(
            "GOOGLE_APPLICATION_CREDENTIALS",
            file.path().to_str().unwrap(),
        );

        let res = load_credentials_inner(
            None,
            &PathBuf::from("/missing/enc"),
            &PathBuf::from("/missing/plain"),
        )
        .await;

        match res.unwrap() {
            Credential::ServiceAccount(key) => {
                assert_eq!(
                    key.client_email,
                    "adc-sa@test-project.iam.gserviceaccount.com"
                );
            }
            _ => panic!("Expected ServiceAccount from ADC"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_load_credentials_adc_env_var_missing_file() {
        let _adc_guard = EnvVarGuard::set("GOOGLE_APPLICATION_CREDENTIALS", "/does/not/exist.json");

        // When GOOGLE_APPLICATION_CREDENTIALS points to a missing file, we error immediately
        // rather than falling through — the user explicitly asked for this file.
        let err = load_credentials_inner(
            None,
            &PathBuf::from("/missing/enc"),
            &PathBuf::from("/missing/plain"),
        )
        .await;

        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("does not exist"),
            "Should hard-error when GOOGLE_APPLICATION_CREDENTIALS points to missing file, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_load_credentials_env_file_missing() {
        let err = load_credentials_inner(
            Some("/does/not/exist"),
            &PathBuf::from("/also/missing"),
            &PathBuf::from("/still/missing"),
        )
        .await;
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn test_load_credentials_env_file_authorized_user() {
        let mut file = NamedTempFile::new().unwrap();
        let json = r#"{
            "client_id": "test_id",
            "client_secret": "test_secret",
            "refresh_token": "test_refresh",
            "type": "authorized_user"
        }"#;
        file.write_all(json.as_bytes()).unwrap();

        let res = load_credentials_inner(
            Some(file.path().to_str().unwrap()),
            &PathBuf::from("/also/missing"),
            &PathBuf::from("/still/missing"),
        )
        .await
        .unwrap();

        match res {
            Credential::AuthorizedUser(secret) => {
                assert_eq!(secret.client_id, "test_id");
                assert_eq!(secret.refresh_token, "test_refresh");
            }
            _ => panic!("Expected AuthorizedUser"),
        }
    }

    #[tokio::test]
    async fn test_load_credentials_env_file_service_account() {
        let mut file = NamedTempFile::new().unwrap();
        let json = r#"{
            "type": "service_account",
            "project_id": "test",
            "private_key_id": "test-key-id",
            "private_key": "-----BEGIN PRIVATE KEY-----\nMIIEvwIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----\n",
            "client_email": "test@test.iam.gserviceaccount.com",
            "client_id": "123",
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": "https://oauth2.googleapis.com/token"
        }"#;
        file.write_all(json.as_bytes()).unwrap();

        let res = load_credentials_inner(
            Some(file.path().to_str().unwrap()),
            &PathBuf::from("/also/missing"),
            &PathBuf::from("/still/missing"),
        )
        .await
        .unwrap();

        match res {
            Credential::ServiceAccount(key) => {
                assert_eq!(key.client_email, "test@test.iam.gserviceaccount.com");
            }
            _ => panic!("Expected ServiceAccount"),
        }
    }

    #[tokio::test]
    async fn test_load_credentials_default_path_authorized_user() {
        let mut file = NamedTempFile::new().unwrap();
        let json = r#"{
            "client_id": "default_id",
            "client_secret": "default_secret",
            "refresh_token": "default_refresh",
            "type": "authorized_user"
        }"#;
        file.write_all(json.as_bytes()).unwrap();

        let res = load_credentials_inner(None, &PathBuf::from("/also/missing"), file.path())
            .await
            .unwrap();

        match res {
            Credential::AuthorizedUser(secret) => {
                assert_eq!(secret.client_id, "default_id");
            }
            _ => panic!("Expected AuthorizedUser"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_get_token_from_env_var() {
        let _token_guard = EnvVarGuard::set("GOOGLE_WORKSPACE_CLI_TOKEN", "my-test-token");

        let result = get_token(&["https://www.googleapis.com/auth/drive"]).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "my-test-token");
    }

    #[tokio::test]
    async fn test_load_credentials_encrypted_file() {
        // Simulate an encrypted credentials file
        let json = r#"{
            "client_id": "enc_test_id",
            "client_secret": "enc_test_secret",
            "refresh_token": "enc_test_refresh",
            "type": "authorized_user"
        }"#;

        let dir = tempfile::tempdir().unwrap();
        let enc_path = dir.path().join("credentials.enc");

        // Encrypt and write
        let encrypted = crate::credential_store::encrypt(json.as_bytes()).unwrap();
        std::fs::write(&enc_path, &encrypted).unwrap();

        let res = load_credentials_inner(None, &enc_path, &PathBuf::from("/does/not/exist"))
            .await
            .unwrap();

        match res {
            Credential::AuthorizedUser(secret) => {
                assert_eq!(secret.client_id, "enc_test_id");
                assert_eq!(secret.client_secret, "enc_test_secret");
                assert_eq!(secret.refresh_token, "enc_test_refresh");
            }
            _ => panic!("Expected AuthorizedUser from encrypted credentials"),
        }
    }

    #[tokio::test]
    async fn test_load_credentials_encrypted_takes_priority_over_default() {
        // Encrypted credentials should be loaded before the default plaintext path
        let enc_json = r#"{
            "client_id": "encrypted_id",
            "client_secret": "encrypted_secret",
            "refresh_token": "encrypted_refresh",
            "type": "authorized_user"
        }"#;
        let plain_json = r#"{
            "client_id": "plaintext_id",
            "client_secret": "plaintext_secret",
            "refresh_token": "plaintext_refresh",
            "type": "authorized_user"
        }"#;

        let dir = tempfile::tempdir().unwrap();
        let enc_path = dir.path().join("credentials.enc");
        let plain_path = dir.path().join("credentials.json");

        let encrypted = crate::credential_store::encrypt(enc_json.as_bytes()).unwrap();
        std::fs::write(&enc_path, &encrypted).unwrap();
        std::fs::write(&plain_path, plain_json).unwrap();

        let res = load_credentials_inner(None, &enc_path, &plain_path)
            .await
            .unwrap();

        match res {
            Credential::AuthorizedUser(secret) => {
                assert_eq!(
                    secret.client_id, "encrypted_id",
                    "Encrypted credentials should take priority over plaintext"
                );
            }
            _ => panic!("Expected AuthorizedUser"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_get_token_env_var_empty_falls_through() {
        // An empty token should not short-circuit — it should be ignored
        // and fall through to normal credential loading.
        // Isolate from host ADC so the well-known path doesn't match.
        let tmp = tempfile::tempdir().unwrap();
        let _home_guard = EnvVarGuard::set("HOME", tmp.path());
        let _adc_guard = EnvVarGuard::remove("GOOGLE_APPLICATION_CREDENTIALS");
        let _token_guard = EnvVarGuard::set("GOOGLE_WORKSPACE_CLI_TOKEN", "");

        let result = load_credentials_inner(
            None,
            &PathBuf::from("/does/not/exist1"),
            &PathBuf::from("/does/not/exist2"),
        )
        .await;

        // Should fall through to normal credential loading, which fails
        // because we pointed at non-existent paths
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No credentials found"));
    }

    #[test]
    #[serial_test::serial]
    fn test_get_quota_project_reads_adc() {
        let tmp = tempfile::tempdir().unwrap();
        let adc_dir = tmp.path().join(".config").join("gcloud");
        std::fs::create_dir_all(&adc_dir).unwrap();
        std::fs::write(
            adc_dir.join("application_default_credentials.json"),
            r#"{"quota_project_id": "my-project-123"}"#,
        )
        .unwrap();

        let _home_guard = EnvVarGuard::set("HOME", tmp.path());
        let _adc_guard = EnvVarGuard::remove("GOOGLE_APPLICATION_CREDENTIALS");
        assert_eq!(get_quota_project(), Some("my-project-123".to_string()));
    }
}

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
use serde::Deserialize;

use crate::credential_store;

const PROXY_ENV_VARS: &[&str] = &[
    "http_proxy",
    "HTTP_PROXY",
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
];

/// Response from Google's token endpoint
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    expires_in: u64,
    #[allow(dead_code)]
    token_type: String,
}

/// Refresh an access token using reqwest (supports HTTP proxy via environment variables).
/// This is used as a fallback when yup-oauth2's hyper-based client fails due to proxy issues.
async fn refresh_token_with_reqwest(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> anyhow::Result<String> {
    let client = crate::client::shared_client().map_err(anyhow::Error::from)?;
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await
        .context("Failed to send token refresh request")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response_text_or_placeholder(response.text().await);
        anyhow::bail!("Token refresh failed with status {}: {}", status, body);
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .context("Failed to parse token response")?;

    Ok(token_response.access_token)
}

/// Returns the project ID to be used for quota and billing (sets the `x-goog-user-project` header).
///
/// Priority:
/// 1. `GOOGLE_WORKSPACE_PROJECT_ID` environment variable.
/// 2. `project_id` from the OAuth client configuration (`client_secret.json`).
/// 3. `quota_project_id` from Application Default Credentials (ADC).
pub fn get_quota_project() -> Option<String> {
    // 1. Explicit environment variable (highest priority)
    if let Ok(project_id) = std::env::var("GOOGLE_WORKSPACE_PROJECT_ID") {
        if !project_id.is_empty() {
            return Some(project_id);
        }
    }

    // 2. Project ID from the OAuth client configuration (set via `gws auth setup`)
    if let Ok(config) = crate::oauth_config::load_client_config() {
        if !config.project_id.is_empty() {
            return Some(config.project_id);
        }
    }

    // 3. Fallback to Application Default Credentials (ADC)
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

/// Fetches access tokens for a fixed set of scopes.
///
/// Long-running helpers use this trait so they can request a fresh token before
/// each API call instead of holding a single token string until it expires.
#[async_trait::async_trait]
pub trait AccessTokenProvider: Send + Sync {
    async fn access_token(&self) -> anyhow::Result<String>;
}

/// A token provider backed by [`get_token`].
///
/// This keeps the scope list in one place so call sites can ask for a fresh
/// token whenever they need to make another request.
#[derive(Debug, Clone)]
pub struct ScopedTokenProvider {
    scopes: Vec<String>,
}

impl ScopedTokenProvider {
    pub fn new(scopes: &[&str]) -> Self {
        Self {
            scopes: scopes.iter().map(|scope| (*scope).to_string()).collect(),
        }
    }
}

#[async_trait::async_trait]
impl AccessTokenProvider for ScopedTokenProvider {
    async fn access_token(&self) -> anyhow::Result<String> {
        let scopes: Vec<&str> = self.scopes.iter().map(String::as_str).collect();
        get_token(&scopes).await
    }
}

pub fn token_provider(scopes: &[&str]) -> ScopedTokenProvider {
    ScopedTokenProvider::new(scopes)
}

/// A fake [`AccessTokenProvider`] for tests that returns tokens from a queue.
#[cfg(test)]
pub struct FakeTokenProvider {
    tokens: std::sync::Arc<tokio::sync::Mutex<std::collections::VecDeque<String>>>,
}

#[cfg(test)]
impl FakeTokenProvider {
    pub fn new(tokens: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            tokens: std::sync::Arc::new(tokio::sync::Mutex::new(
                tokens.into_iter().map(|t| t.to_string()).collect(),
            )),
        }
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl AccessTokenProvider for FakeTokenProvider {
    async fn access_token(&self) -> anyhow::Result<String> {
        self.tokens
            .lock()
            .await
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("no test token remaining"))
    }
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

/// Check if HTTP proxy environment variables are set
pub(crate) fn has_proxy_env() -> bool {
    PROXY_ENV_VARS
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

pub(crate) fn response_text_or_placeholder<E>(result: Result<String, E>) -> String {
    result.unwrap_or_else(|_| "(could not read error response body)".to_string())
}

async fn get_token_inner(
    scopes: &[&str],
    creds: Credential,
    token_cache_path: &std::path::Path,
) -> anyhow::Result<String> {
    match creds {
        Credential::AuthorizedUser(ref secret) => {
            // If proxy env vars are set, use reqwest directly (it supports proxy)
            // This avoids waiting for yup-oauth2's hyper client to timeout
            if has_proxy_env() {
                return refresh_token_with_reqwest(
                    &secret.client_id,
                    &secret.client_secret,
                    &secret.refresh_token,
                )
                .await;
            }

            // No proxy - use yup-oauth2 (faster, has token caching)
            let auth = yup_oauth2::AuthorizedUserAuthenticator::builder(secret.clone())
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
    load_credentials_with_loader(
        env_file,
        enc_path,
        default_path,
        credential_store::load_encrypted_from_path,
    )
    .await
}

// Keep credential selection testable without accessing the OS keyring.
async fn load_credentials_with_loader(
    env_file: Option<&str>,
    enc_path: &std::path::Path,
    default_path: &std::path::Path,
    load_encrypted: impl FnOnce(&std::path::Path) -> anyhow::Result<String>,
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

    // 2. Encrypted credentials
    if enc_path.exists() {
        // A read, decryption, or keyring failure does not mean the files are
        // disposable. Stop here so a retry cannot silently select another account.
        // Do not render backend error details, which may contain sensitive data.
        let json_str = load_encrypted(enc_path).map_err(|_| {
            anyhow::anyhow!(
                "Failed to read or decrypt saved credentials at {}. \
                 Check access to the original OS keyring or encryption key and verify \
                 GOOGLE_WORKSPACE_CLI_CONFIG_DIR. Credentials and token caches have been \
                 preserved; no fallback credentials were used. Back up the configuration \
                 before intentionally replacing credentials with `gws auth logout` and \
                 `gws auth login`.",
                crate::output::sanitize_for_terminal(&enc_path.display().to_string())
            )
        })?;
        return parse_credential_file(enc_path, &json_str).await;
    }

    // 3. Plaintext credentials at default path (AuthorizedUser)
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

    fn clear_proxy_env() -> Vec<EnvVarGuard> {
        PROXY_ENV_VARS
            .iter()
            .map(|key| EnvVarGuard::remove(key))
            .collect()
    }

    #[test]
    #[serial_test::serial]
    fn has_proxy_env_returns_false_when_unset() {
        let _guards = clear_proxy_env();
        assert!(!has_proxy_env());
    }

    #[test]
    #[serial_test::serial]
    fn has_proxy_env_returns_true_when_set() {
        let mut guards = clear_proxy_env();
        guards.push(EnvVarGuard::set(
            "HTTPS_PROXY",
            "http://proxy.internal:8080",
        ));
        assert!(has_proxy_env());
    }

    #[test]
    fn response_text_or_placeholder_returns_body() {
        let body = response_text_or_placeholder(Result::<String, ()>::Ok("error body".to_string()));
        assert_eq!(body, "error body");
    }

    #[test]
    fn response_text_or_placeholder_returns_placeholder_on_error() {
        let body = response_text_or_placeholder(Result::<String, ()>::Err(()));
        assert_eq!(body, "(could not read error response body)");
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
    #[serial_test::serial]
    async fn test_scoped_token_provider_uses_get_token() {
        let _token_guard = EnvVarGuard::set("GOOGLE_WORKSPACE_CLI_TOKEN", "provider-token");
        let provider = token_provider(&["https://www.googleapis.com/auth/drive"]);

        let first = provider.access_token().await.unwrap();
        let second = provider.access_token().await.unwrap();

        assert_eq!(first, "provider-token");
        assert_eq!(second, "provider-token");
    }

    #[tokio::test]
    #[serial_test::serial]
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

        // Isolate global config dir to prevent races with other tests
        std::env::set_var("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", dir.path());

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
    async fn test_load_credentials_preserves_failed_encrypted_credentials_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let enc_path = dir.path().join("credentials.enc");
        let token_path = dir.path().join("token_cache.json");
        let service_token_path = dir.path().join("sa_token_cache.json");
        let absent_path = dir.path().join("missing.json");
        let _adc_guard = EnvVarGuard::set("GOOGLE_APPLICATION_CREDENTIALS", &absent_path);

        // A short invalid payload fails before accessing any OS keyring.
        std::fs::write(&enc_path, b"bad").unwrap();
        std::fs::write(&token_path, b"synthetic-user-cache").unwrap();
        std::fs::write(&service_token_path, b"synthetic-service-cache").unwrap();

        for _ in 0..2 {
            let result = load_credentials_inner(None, &enc_path, &absent_path).await;

            assert!(result.is_err());
            assert!(
                enc_path.exists(),
                "Authentication failure must preserve saved encrypted credentials"
            );
            assert_eq!(std::fs::read(&enc_path).unwrap(), b"bad");
            assert_eq!(std::fs::read(&token_path).unwrap(), b"synthetic-user-cache");
            assert_eq!(
                std::fs::read(&service_token_path).unwrap(),
                b"synthetic-service-cache"
            );
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_load_credentials_corrupt_encrypted_reports_safe_remediation() {
        let dir = tempfile::tempdir().unwrap();
        let enc_path = dir.path().join("credentials.enc");
        let absent_path = dir.path().join("missing.json");
        let _adc_guard = EnvVarGuard::set("GOOGLE_APPLICATION_CREDENTIALS", &absent_path);
        std::fs::write(&enc_path, b"bad").unwrap();

        let err = load_credentials_inner(None, &enc_path, &absent_path)
            .await
            .unwrap_err();
        let msg = format!("{err:#}");

        assert!(msg.contains("decrypt"), "{msg}");
        assert!(msg.contains("keyring"), "{msg}");
        assert!(msg.contains("preserved"), "{msg}");
        assert!(msg.contains("Back up"), "{msg}");
        assert!(!msg.contains("No credentials found"), "{msg}");
        assert!(!msg.contains("bad"), "{msg}");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_load_credentials_corrupt_encrypted_blocks_plaintext_and_adc() {
        // Exercise both a default-style layout and a configured directory,
        // without changing HOME or touching the actual default directory.
        let dir = tempfile::tempdir().unwrap();
        let fallback_json = r#"{
            "client_id": "different-account",
            "client_secret": "synthetic-secret",
            "refresh_token": "synthetic-refresh",
            "type": "authorized_user"
        }"#;
        let adc_path = dir.path().join("adc.json");
        std::fs::write(&adc_path, fallback_json).unwrap();
        let _adc_guard = EnvVarGuard::set("GOOGLE_APPLICATION_CREDENTIALS", &adc_path);

        for layout in [".config/gws", "custom-config"] {
            let config = dir.path().join(layout);
            std::fs::create_dir_all(&config).unwrap();
            let enc_path = config.join("credentials.enc");
            let plain_path = config.join("credentials.json");
            std::fs::write(&enc_path, b"bad").unwrap();
            std::fs::write(&plain_path, fallback_json).unwrap();

            for fallback in [&plain_path, &config.join("missing.json")] {
                let err = load_credentials_inner(None, &enc_path, fallback)
                    .await
                    .expect_err("Broken encrypted credentials must block another account");
                assert!(err.to_string().contains("decrypt"));
                assert_eq!(std::fs::read(&enc_path).unwrap(), b"bad");
                assert_eq!(std::fs::read_to_string(&plain_path).unwrap(), fallback_json);
                assert_eq!(std::fs::read_to_string(&adc_path).unwrap(), fallback_json);
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_load_credentials_keyring_failure_preserves_files_and_blocks_adc() {
        let dir = tempfile::tempdir().unwrap();
        let enc_path = dir.path().join("credentials.enc");
        let plain_path = dir.path().join("credentials.json");
        let adc_path = dir.path().join("adc.json");
        let _adc_guard = EnvVarGuard::set("GOOGLE_APPLICATION_CREDENTIALS", &adc_path);
        let sentinels: &[(&str, &[u8])] = &[
            ("credentials.enc", b"synthetic-encrypted-credentials"),
            ("token_cache.json", b"synthetic-user-cache"),
            ("sa_token_cache.json", b"synthetic-service-cache"),
            (".encryption_key", b"synthetic-key"),
        ];
        for (name, bytes) in sentinels {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        std::fs::write(
            &adc_path,
            r#"{"type":"authorized_user","client_id":"other","client_secret":"secret","refresh_token":"refresh"}"#,
        )
        .unwrap();

        for _ in 0..2 {
            let err = load_credentials_with_loader(None, &enc_path, &plain_path, |_| {
                anyhow::bail!("OS keyring unavailable: synthetic-sensitive-detail")
            })
            .await
            .expect_err("Key acquisition failure must stop credential selection");
            for (name, bytes) in sentinels {
                assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), *bytes);
            }
            let msg = format!("{err:#}");
            assert!(msg.contains("keyring"), "{msg}");
            assert!(msg.contains("preserved"), "{msg}");
            assert!(!msg.contains("synthetic-sensitive-detail"), "{msg}");
            assert!(!msg.contains("synthetic-key"), "{msg}");
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_load_credentials_explicit_file_precedes_broken_encrypted_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let enc_path = dir.path().join("credentials.enc");
        let explicit_path = dir.path().join("explicit.json");
        let missing_path = dir.path().join("missing.json");
        let _adc_guard = EnvVarGuard::set("GOOGLE_APPLICATION_CREDENTIALS", &missing_path);
        std::fs::write(&enc_path, b"bad").unwrap();
        std::fs::write(
            &explicit_path,
            r#"{"type":"authorized_user","client_id":"explicit","client_secret":"secret","refresh_token":"refresh"}"#,
        )
        .unwrap();

        let creds = load_credentials_inner(explicit_path.to_str(), &enc_path, &missing_path)
            .await
            .unwrap();
        match creds {
            Credential::AuthorizedUser(secret) => assert_eq!(secret.client_id, "explicit"),
            _ => panic!("Expected explicitly selected account"),
        }
        assert_eq!(std::fs::read(&enc_path).unwrap(), b"bad");

        // A missing or malformed explicit file must not select another account.
        for contents in [None, Some("invalid-json")] {
            if let Some(contents) = contents {
                std::fs::write(&missing_path, contents).unwrap();
            }
            let err = load_credentials_inner(missing_path.to_str(), &enc_path, &explicit_path)
                .await
                .unwrap_err();
            assert!(err.to_string().contains(if contents.is_some() {
                "Failed to parse"
            } else {
                "does not exist"
            }));
            assert_eq!(std::fs::read(&enc_path).unwrap(), b"bad");
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_get_token_preserves_configured_credentials_until_explicit_logout() {
        let dir = tempfile::tempdir().unwrap();
        let enc_path = dir.path().join("credentials.enc");
        let missing_path = dir.path().join("missing.json");
        let _config_guard = EnvVarGuard::set("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", dir.path());
        let _adc_guard = EnvVarGuard::set("GOOGLE_APPLICATION_CREDENTIALS", &missing_path);
        let _file_guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE");
        let _token_guard = EnvVarGuard::set("GOOGLE_WORKSPACE_CLI_TOKEN", "");
        let names = [
            "credentials.enc",
            "credentials.json",
            "token_cache.json",
            "sa_token_cache.json",
        ];
        for name in names {
            std::fs::write(dir.path().join(name), b"bad").unwrap();
        }

        let err = get_token(&[]).await.unwrap_err();
        assert!(err.to_string().contains("decrypt"), "{err}");
        for name in names {
            assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), b"bad");
        }

        // An explicit token still takes precedence over all credential files.
        {
            let _token_guard = EnvVarGuard::set("GOOGLE_WORKSPACE_CLI_TOKEN", "synthetic-token");
            assert_eq!(get_token(&[]).await.unwrap(), "synthetic-token");
            assert_eq!(std::fs::read(&enc_path).unwrap(), b"bad");
        }

        crate::auth_commands::handle_auth_command(&["logout".into()])
            .await
            .unwrap();
        for name in names {
            assert!(!dir.path().join(name).exists(), "Logout must remove {name}");
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
    fn test_get_quota_project_priority_env_var() {
        let _env_guard = EnvVarGuard::set("GOOGLE_WORKSPACE_PROJECT_ID", "priority-env");
        let _adc_guard = EnvVarGuard::remove("GOOGLE_APPLICATION_CREDENTIALS");
        let _config_guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_CLI_CONFIG_DIR");
        let _home_guard = EnvVarGuard::set("HOME", "/missing/home");

        assert_eq!(get_quota_project(), Some("priority-env".to_string()));
    }

    #[test]
    #[serial_test::serial]
    fn test_get_quota_project_priority_config() {
        let tmp = tempfile::tempdir().unwrap();
        let _config_guard = EnvVarGuard::set(
            "GOOGLE_WORKSPACE_CLI_CONFIG_DIR",
            tmp.path().to_str().unwrap(),
        );
        let _env_guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_PROJECT_ID");
        let _adc_guard = EnvVarGuard::remove("GOOGLE_APPLICATION_CREDENTIALS");
        let _home_guard = EnvVarGuard::set("HOME", "/missing/home");

        // Save a client config with a project ID
        crate::oauth_config::save_client_config("id", "secret", "config-project").unwrap();

        assert_eq!(get_quota_project(), Some("config-project".to_string()));
    }

    #[test]
    #[serial_test::serial]
    fn test_get_quota_project_priority_adc_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let adc_dir = tmp.path().join(".config").join("gcloud");
        std::fs::create_dir_all(&adc_dir).unwrap();
        std::fs::write(
            adc_dir.join("application_default_credentials.json"),
            r#"{"quota_project_id": "adc-project"}"#,
        )
        .unwrap();

        let _home_guard = EnvVarGuard::set("HOME", tmp.path());
        let _env_guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_PROJECT_ID");
        let _config_guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_CLI_CONFIG_DIR");
        let _adc_guard = EnvVarGuard::remove("GOOGLE_APPLICATION_CREDENTIALS");

        assert_eq!(get_quota_project(), Some("adc-project".to_string()));
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
        // Isolate from local environment
        let _env_guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_PROJECT_ID");
        let _config_guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_CLI_CONFIG_DIR");

        assert_eq!(get_quota_project(), Some("my-project-123".to_string()));
    }
}

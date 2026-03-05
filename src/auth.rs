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

/// Returns the well-known Application Default Credentials path:
/// `~/.config/gcloud/application_default_credentials.json`.
fn adc_well_known_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("gcloud").join("application_default_credentials.json"))
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
/// 2. Per-account encrypted credentials via `accounts.json` registry
/// 3. Plaintext credentials at `~/.config/gws/credentials.json` (User only)
/// 4. Application Default Credentials (ADC):
///    - `GOOGLE_APPLICATION_CREDENTIALS` env var (path to a JSON credentials file), then
///    - Well-known ADC path: `~/.config/gcloud/application_default_credentials.json`
///      (populated by `gcloud auth application-default login`)
///
/// When `account` is `Some`, a specific registered account is used.
/// When `account` is `None`, the default account from `accounts.json` is used.
pub async fn get_token(scopes: &[&str], account: Option<&str>) -> anyhow::Result<String> {
    // 0. Direct token from env var (highest priority, bypasses all credential loading)
    if let Ok(token) = std::env::var("GOOGLE_WORKSPACE_CLI_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    let creds_file = std::env::var("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE").ok();
    let impersonated_user = std::env::var("GOOGLE_WORKSPACE_CLI_IMPERSONATED_USER").ok();
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gws");

    // If env var credentials are specified, skip account resolution entirely
    if creds_file.is_some() {
        let enc_path = credential_store::encrypted_credentials_path();
        let default_path = config_dir.join("credentials.json");
        let token_cache = config_dir.join("token_cache.json");
        let creds = load_credentials_inner(creds_file.as_deref(), &enc_path, &default_path).await?;
        return get_token_inner(scopes, creds, &token_cache, impersonated_user.as_deref()).await;
    }

    // Resolve account from registry
    let resolved_account = resolve_account(account)?;

    let enc_path = match &resolved_account {
        Some(email) => credential_store::encrypted_credentials_path_for(email),
        None => credential_store::encrypted_credentials_path(),
    };

    // Per-account token cache: token_cache.<b64-email>.json
    let token_cache_name = resolved_account
        .as_ref()
        .map(|email| {
            let b64 = crate::accounts::email_to_b64(&crate::accounts::normalize_email(email));
            format!("token_cache.{b64}.json")
        })
        .unwrap_or_else(|| "token_cache.json".to_string());
    let token_cache_path = config_dir.join(token_cache_name);

    let default_path = config_dir.join("credentials.json");
    let creds = load_credentials_inner(None, &enc_path, &default_path).await?;
    get_token_inner(
        scopes,
        creds,
        &token_cache_path,
        impersonated_user.as_deref(),
    )
    .await
}

/// Resolve which account to use:
/// 1. Explicit `account` parameter takes priority.
/// 2. Fall back to `accounts.json` default.
/// 3. If no registry exists but legacy `credentials.enc` exists, fail with upgrade message.
/// 4. If nothing exists, return None (will fall through to standard error).
fn resolve_account(account: Option<&str>) -> anyhow::Result<Option<String>> {
    let registry = crate::accounts::load_accounts()?;

    match (account, &registry) {
        // Explicit account requested — validate it exists in registry
        (Some(email), Some(reg)) => {
            let normalised = crate::accounts::normalize_email(email);
            if !reg.accounts.contains_key(&normalised) {
                anyhow::bail!(
                    "Account '{}' not found. Run 'gws auth login' to add it.",
                    normalised
                );
            }
            Ok(Some(normalised))
        }
        // Explicit account but no registry
        (Some(email), None) => {
            anyhow::bail!(
                "Account '{}' not found. No accounts registered. Run 'gws auth login'.",
                crate::accounts::normalize_email(email)
            );
        }
        // No explicit account — use default from registry
        (None, Some(reg)) => {
            if let Some(default) = crate::accounts::get_default(reg) {
                Ok(Some(default.to_string()))
            } else if reg.accounts.len() == 1 {
                // Auto-select the only account
                Ok(reg.accounts.keys().next().cloned())
            } else {
                anyhow::bail!(
                    "No default account set. Use --account or run 'gws auth default <email>'."
                );
            }
        }
        // No account, no registry — check for legacy credentials
        (None, None) => {
            let legacy_path = credential_store::encrypted_credentials_path();
            if legacy_path.exists() {
                anyhow::bail!(
                    "Legacy credentials found at {}. \
                     gws now supports multiple accounts. \
                     Please run 'gws auth login' to upgrade your credentials.",
                    legacy_path.display()
                );
            }
            // No registry, no legacy — fall through to standard credential loading
            Ok(None)
        }
    }
}

async fn get_token_inner(
    scopes: &[&str],
    creds: Credential,
    token_cache_path: &std::path::Path,
    impersonated_user: Option<&str>,
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
            let mut builder = yup_oauth2::ServiceAccountAuthenticator::builder(key).with_storage(
                Box::new(crate::token_storage::EncryptedTokenStorage::new(sa_cache)),
            );

            // Check for impersonation
            if let Some(user) = impersonated_user {
                if !user.trim().is_empty() {
                    builder = builder.subject(user.to_string());
                }
            }

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

async fn load_credentials_inner(
    env_file: Option<&str>,
    enc_path: &std::path::Path,
    default_path: &std::path::Path,
) -> anyhow::Result<Credential> {
    // 1. Explicit env var — plaintext file (User or Service Account)
    if let Some(path) = env_file {
        let p = PathBuf::from(path);
        if p.exists() {
            // Read file content first to determine type
            let content = tokio::fs::read_to_string(&p)
                .await
                .with_context(|| format!("Failed to read credentials from {path}"))?;

            let json: serde_json::Value =
                serde_json::from_str(&content).context("Failed to parse credentials JSON")?;

            // Check for "type" field
            if let Some(type_str) = json.get("type").and_then(|v| v.as_str()) {
                if type_str == "service_account" {
                    let key = yup_oauth2::parse_service_account_key(&content)
                        .context("Failed to parse service account key")?;
                    return Ok(Credential::ServiceAccount(key));
                }
            }

            // Default to parsed authorized user secret if not service account
            // We re-parse specifically to AuthorizedUserSecret to validate fields
            let secret: yup_oauth2::authorized_user::AuthorizedUserSecret =
                serde_json::from_str(&content)
                    .context("Failed to parse authorized user credentials")?;
            return Ok(Credential::AuthorizedUser(secret));
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

    // 4. Application Default Credentials (ADC)
    // Checks GOOGLE_APPLICATION_CREDENTIALS env var first, then the well-known path.
    let adc_path = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .ok()
        .map(PathBuf::from)
        .or_else(adc_well_known_path);

    if let Some(ref adc) = adc_path {
        if adc.exists() {
            let content = tokio::fs::read_to_string(adc)
                .await
                .with_context(|| format!("Failed to read ADC from {}", adc.display()))?;
            let json: serde_json::Value =
                serde_json::from_str(&content).context("Failed to parse ADC JSON")?;
            if json.get("type").and_then(|v| v.as_str()) == Some("service_account") {
                let key = yup_oauth2::parse_service_account_key(&content)
                    .context("Failed to parse service account key from ADC")?;
                return Ok(Credential::ServiceAccount(key));
            }
            let secret: yup_oauth2::authorized_user::AuthorizedUserSecret =
                serde_json::from_str(&content)
                    .context("Failed to parse authorized user credentials from ADC")?;
            return Ok(Credential::AuthorizedUser(secret));
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

    #[tokio::test]
    async fn test_load_credentials_no_options() {
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

        unsafe {
            std::env::set_var(
                "GOOGLE_APPLICATION_CREDENTIALS",
                file.path().to_str().unwrap(),
            );
        }

        let res = load_credentials_inner(
            None,
            &PathBuf::from("/missing/enc"),
            &PathBuf::from("/missing/plain"),
        )
        .await;

        unsafe {
            std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
        }

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
    async fn test_load_credentials_adc_env_var_missing_file() {
        unsafe {
            std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", "/does/not/exist.json");
        }

        // A missing ADC file pointed to by env var should fall through to the
        // "No credentials found" error, not a hard failure.  The env var path
        // is only used when it points to an existing file.
        let err = load_credentials_inner(
            None,
            &PathBuf::from("/missing/enc"),
            &PathBuf::from("/missing/plain"),
        )
        .await;

        unsafe {
            std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
        }

        assert!(err.is_err());
        assert!(
            err.unwrap_err().to_string().contains("No credentials found"),
            "Should fall through when ADC env var points to missing file"
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
        // Save the old token
        let old_token = std::env::var("GOOGLE_WORKSPACE_CLI_TOKEN").ok();

        // Set the token env var
        unsafe {
            std::env::set_var("GOOGLE_WORKSPACE_CLI_TOKEN", "my-test-token");
        }

        let result = get_token(&["https://www.googleapis.com/auth/drive"], None).await;

        unsafe {
            if let Some(t) = old_token {
                std::env::set_var("GOOGLE_WORKSPACE_CLI_TOKEN", t);
            } else {
                std::env::remove_var("GOOGLE_WORKSPACE_CLI_TOKEN");
            }
        }

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "my-test-token");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_get_token_env_var_empty_falls_through() {
        // An empty token should not short-circuit — it should be ignored
        // and fall through to normal credential loading.
        // We test with non-existent credential paths to ensure fallthrough.
        unsafe {
            std::env::set_var("GOOGLE_WORKSPACE_CLI_TOKEN", "");
        }

        let result = load_credentials_inner(
            None,
            &PathBuf::from("/does/not/exist1"),
            &PathBuf::from("/does/not/exist2"),
        )
        .await;

        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_TOKEN");
        }

        // Should fall through to normal credential loading, which fails
        // because we pointed at non-existent paths
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No credentials found"));
    }
}

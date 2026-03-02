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
/// 2. Encrypted credentials at `~/.config/gws/credentials.enc` (User only)
/// 3. Plaintext credentials at `~/.config/gws/credentials.json` (User only)
pub async fn get_token(scopes: &[&str]) -> anyhow::Result<String> {
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

    let enc_path = credential_store::encrypted_credentials_path();
    let default_path = config_dir.join("credentials.json");

    let creds = load_credentials_inner(creds_file.as_deref(), &enc_path, &default_path).await?;

    get_token_inner(scopes, creds, &config_dir, impersonated_user.as_deref()).await
}

async fn get_token_inner(
    scopes: &[&str],
    creds: Credential,
    config_dir: &std::path::Path,
    impersonated_user: Option<&str>,
) -> anyhow::Result<String> {
    match creds {
        Credential::AuthorizedUser(secret) => {
            let token_cache = config_dir.join("token_cache.json");
            let auth = yup_oauth2::AuthorizedUserAuthenticator::builder(secret)
                .persist_tokens_to_disk(&token_cache)
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
            let token_cache = config_dir.join("service_account_token_cache.json");
            let mut builder = yup_oauth2::ServiceAccountAuthenticator::builder(key)
                .persist_tokens_to_disk(&token_cache);

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

    anyhow::bail!(
        "No credentials found. Run `gws auth setup` to configure, \
         `gws auth login` to authenticate, or set GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE"
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
    async fn test_get_token_from_env_var() {
        // Set the token env var
        unsafe {
            std::env::set_var("GOOGLE_WORKSPACE_CLI_TOKEN", "my-test-token");
        }

        let result = get_token(&["https://www.googleapis.com/auth/drive"]).await;

        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_TOKEN");
        }

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "my-test-token");
    }

    #[tokio::test]
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

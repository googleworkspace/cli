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

//! Helpers for the OAuth client configuration file.
//!
//! Uses the standard Google Cloud Console "installed application" JSON format:
//! ```json
//! {
//!   "installed": {
//!     "client_id": "...apps.googleusercontent.com",
//!     "project_id": "my-project",
//!     "auth_uri": "https://accounts.google.com/o/oauth2/auth",
//!     "token_uri": "https://oauth2.googleapis.com/token",
//!     "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
//!     "client_secret": "GOCSPX-...",
//!     "redirect_uris": ["http://localhost"]
//!   }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The "installed" application config from Google Cloud Console.
#[derive(Debug, Serialize, Deserialize)]
pub struct InstalledConfig {
    pub client_id: String,
    pub client_secret: String,
    pub project_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    #[serde(default)]
    pub auth_provider_x509_cert_url: String,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
}

/// Wrapper matching the Google Cloud Console download format.
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientSecretFile {
    pub installed: InstalledConfig,
}

/// Returns the path for the client secret config file.
pub fn client_config_path() -> PathBuf {
    crate::auth_commands::config_dir().join("client_secret.json")
}

/// Wrappers the Cloud Console uses around the installed-app credentials.
const CLIENT_DOCUMENT_SCOPES: [&str; 2] = ["installed", "web"];

/// Recovers `client_id`/`client_secret` from a pasted `client_secret.json` document.
///
/// The Console only offers the OAuth client as a downloadable document, so users
/// paste that whole document into the setup prompts. Storing it verbatim wrote a
/// double-serialized config whose `client_id` was the document itself, which every
/// later login rejects with `401 invalid_client`.
pub fn extract_client_credentials(value: &str) -> Option<(String, String)> {
    let parsed: serde_json::Value = serde_json::from_str(value).ok()?;
    let document = parsed.as_object()?;
    for scope in CLIENT_DOCUMENT_SCOPES {
        let Some(credentials) = document.get(scope).and_then(|value| value.as_object()) else {
            continue;
        };
        let (Some(raw_id), Some(raw_secret)) = (
            credentials
                .get("client_id")
                .and_then(|value| value.as_str()),
            credentials
                .get("client_secret")
                .and_then(|value| value.as_str()),
        ) else {
            continue;
        };
        let (client_id, client_secret) = (raw_id.trim(), raw_secret.trim());
        if !client_id.is_empty() && !client_secret.is_empty() {
            return Some((client_id.to_string(), client_secret.to_string()));
        }
    }
    None
}

/// Saves OAuth client configuration in the standard Google Cloud Console format.
pub fn save_client_config(
    client_id: &str,
    client_secret: &str,
    project_id: &str,
) -> anyhow::Result<PathBuf> {
    // A pasted document carries both halves of the pair, so it wins whichever
    // prompt it arrived in; plain values are stored exactly as given.
    let (client_id, client_secret) = match extract_client_credentials(client_id)
        .or_else(|| extract_client_credentials(client_secret))
    {
        Some((pasted_id, pasted_secret)) => (pasted_id, pasted_secret),
        None => (client_id.to_string(), client_secret.to_string()),
    };

    let config = ClientSecretFile {
        installed: InstalledConfig {
            client_id,
            client_secret,
            project_id: project_id.to_string(),
            auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            auth_provider_x509_cert_url: "https://www.googleapis.com/oauth2/v1/certs".to_string(),
            redirect_uris: vec!["http://localhost".to_string()],
        },
    };

    let path = client_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(&config)?;
    crate::fs_util::atomic_write(&path, json.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to write client config: {e}"))?;

    Ok(path)
}

/// Loads OAuth client configuration from the standard Google Cloud Console format.
pub fn load_client_config() -> anyhow::Result<InstalledConfig> {
    let path = client_config_path();
    let data = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {e}", path.display()))?;
    let file: ClientSecretFile = serde_json::from_str(&data)
        .map_err(|e| anyhow::anyhow!("Invalid client_secret.json format: {e}"))?;
    Ok(file.installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client_secret.json");

        let config = ClientSecretFile {
            installed: InstalledConfig {
                client_id: "test-id.apps.googleusercontent.com".to_string(),
                client_secret: "GOCSPX-test".to_string(),
                project_id: "my-project".to_string(),
                auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
                token_uri: "https://oauth2.googleapis.com/token".to_string(),
                auth_provider_x509_cert_url: "https://www.googleapis.com/oauth2/v1/certs"
                    .to_string(),
                redirect_uris: vec!["http://localhost".to_string()],
            },
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &json).unwrap();

        let data = std::fs::read_to_string(&path).unwrap();
        let loaded: ClientSecretFile = serde_json::from_str(&data).unwrap();

        assert_eq!(
            loaded.installed.client_id,
            "test-id.apps.googleusercontent.com"
        );
        assert_eq!(loaded.installed.client_secret, "GOCSPX-test");
        assert_eq!(loaded.installed.project_id, "my-project");
    }

    #[test]
    fn test_parse_google_console_format() {
        // Real format from Google Cloud Console download
        let json = r#"{
            "installed": {
                "client_id": "test-client-id.apps.googleusercontent.com",
                "project_id": "test-project-id",
                "auth_uri": "https://accounts.google.com/o/oauth2/auth",
                "token_uri": "https://oauth2.googleapis.com/token",
                "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
                "client_secret": "test-client-secret",
                "redirect_uris": ["http://localhost"]
            }
        }"#;

        let config: ClientSecretFile = serde_json::from_str(json).unwrap();
        assert_eq!(config.installed.project_id, "test-project-id");
        assert_eq!(config.installed.client_secret, "test-client-secret");
        assert_eq!(config.installed.redirect_uris, vec!["http://localhost"]);
    }

    #[test]
    fn test_parse_missing_optional_fields() {
        // Minimal format — only required fields
        let json = r#"{
            "installed": {
                "client_id": "test-id",
                "project_id": "test-project",
                "auth_uri": "https://accounts.google.com/o/oauth2/auth",
                "token_uri": "https://oauth2.googleapis.com/token",
                "client_secret": "secret"
            }
        }"#;

        let config: ClientSecretFile = serde_json::from_str(json).unwrap();
        assert_eq!(config.installed.client_id, "test-id");
        assert!(config.installed.redirect_uris.is_empty());
        assert!(config.installed.auth_provider_x509_cert_url.is_empty());
    }

    #[test]
    fn test_parse_invalid_json_fails() {
        let json = r#"{ "wrong_key": {} }"#;
        let result = serde_json::from_str::<ClientSecretFile>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_client_id_fails() {
        let json = r#"{
            "installed": {
                "project_id": "test",
                "auth_uri": "https://example.com",
                "token_uri": "https://example.com",
                "client_secret": "secret"
            }
        }"#;
        let result = serde_json::from_str::<ClientSecretFile>(json);
        assert!(result.is_err());
    }

    // Helper to manage the env var safely and clean up automatically
    struct EnvGuard {
        key: String,
        original_value: Option<String>,
    }

    impl EnvGuard {
        fn new(key: &str, value: &str) -> Self {
            let original_value = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key: key.to_string(),
                original_value,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(val) = &self.original_value {
                std::env::set_var(&self.key, val);
            } else {
                std::env::remove_var(&self.key);
            }
        }
    }

    // A user pasting the downloaded client_secret.json into the setup prompts.
    const PASTED_DOCUMENT: &str = r#"{
        "installed": {
            "client_id": "123-abc.apps.googleusercontent.com",
            "project_id": "pasted-project",
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": "https://oauth2.googleapis.com/token",
            "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
            "client_secret": "GOCSPX-pasted",
            "redirect_uris": ["http://localhost"]
        }
    }"#;

    #[test]
    fn test_extract_client_credentials_from_installed_document() {
        let (client_id, client_secret) = extract_client_credentials(PASTED_DOCUMENT).unwrap();
        assert_eq!(client_id, "123-abc.apps.googleusercontent.com");
        assert_eq!(client_secret, "GOCSPX-pasted");
    }

    #[test]
    fn test_extract_client_credentials_from_web_document() {
        let document = r#"{
            "web": {
                "client_id": "999-web.apps.googleusercontent.com",
                "client_secret": "GOCSPX-web"
            }
        }"#;
        let (client_id, client_secret) = extract_client_credentials(document).unwrap();
        assert_eq!(client_id, "999-web.apps.googleusercontent.com");
        assert_eq!(client_secret, "GOCSPX-web");
    }

    #[test]
    fn test_extract_client_credentials_ignores_other_input() {
        assert!(extract_client_credentials("123-abc.apps.googleusercontent.com").is_none());
        assert!(extract_client_credentials("").is_none());
        assert!(extract_client_credentials(r#"{"foo": "bar"}"#).is_none());
        // A document missing the secret cannot supply the pair.
        assert!(extract_client_credentials(r#"{"installed": {"client_id": "only-id"}}"#).is_none());
        // An empty secret is not a usable credential either.
        assert!(extract_client_credentials(
            r#"{"installed": {"client_id": "id", "client_secret": "  "}}"#
        )
        .is_none());
    }

    #[test]
    #[serial_test::serial]
    fn test_save_expands_pasted_client_document() {
        let dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::new(
            "GOOGLE_WORKSPACE_CLI_CONFIG_DIR",
            dir.path().to_str().unwrap(),
        );

        save_client_config(PASTED_DOCUMENT, PASTED_DOCUMENT, "my-project").unwrap();

        let config = load_client_config().unwrap();
        assert_eq!(config.client_id, "123-abc.apps.googleusercontent.com");
        assert_eq!(config.client_secret, "GOCSPX-pasted");
        assert_eq!(config.project_id, "my-project");

        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(client_config_path()).unwrap()).unwrap();
        assert_eq!(
            stored["installed"]["client_id"],
            "123-abc.apps.googleusercontent.com"
        );
        assert_eq!(stored["installed"]["client_secret"], "GOCSPX-pasted");
    }

    #[test]
    #[serial_test::serial]
    fn test_save_keeps_plain_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::new(
            "GOOGLE_WORKSPACE_CLI_CONFIG_DIR",
            dir.path().to_str().unwrap(),
        );

        save_client_config("123-abc.apps.googleusercontent.com", "GOCSPX-plain", "p").unwrap();

        let config = load_client_config().unwrap();
        assert_eq!(config.client_id, "123-abc.apps.googleusercontent.com");
        assert_eq!(config.client_secret, "GOCSPX-plain");
        assert_eq!(config.project_id, "p");
    }

    #[test]
    #[serial_test::serial]
    fn test_load_client_config() {
        let dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::new(
            "GOOGLE_WORKSPACE_CLI_CONFIG_DIR",
            dir.path().to_str().unwrap(),
        );

        // Initially no config file exists
        let result = load_client_config();
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Cannot read"));

        // Create a valid config file
        save_client_config("test-id", "test-secret", "test-project").unwrap();

        // Now loading should succeed
        let config = load_client_config().unwrap();
        assert_eq!(config.client_id, "test-id");
        assert_eq!(config.client_secret, "test-secret");
        assert_eq!(config.project_id, "test-project");

        // Create an invalid config file
        let path = client_config_path();
        std::fs::write(&path, "invalid json").unwrap();

        let result = load_client_config();
        let err = result.unwrap_err();
        assert!(err
            .to_string()
            .contains("Invalid client_secret.json format"));
    }
}

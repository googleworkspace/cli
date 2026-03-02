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

/// Saves OAuth client configuration in the standard Google Cloud Console format.
pub fn save_client_config(
    client_id: &str,
    client_secret: &str,
    project_id: &str,
) -> anyhow::Result<PathBuf> {
    let config = ClientSecretFile {
        installed: InstalledConfig {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            project_id: project_id.to_string(),
            auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            auth_provider_x509_cert_url:
                "https://www.googleapis.com/oauth2/v1/certs".to_string(),
            redirect_uris: vec!["http://localhost".to_string()],
        },
    };

    let path = client_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(&config)?;
    std::fs::write(&path, &json)?;

    // Set file permissions to 600 on Unix (contains secrets)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

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
                auth_provider_x509_cert_url:
                    "https://www.googleapis.com/oauth2/v1/certs".to_string(),
                redirect_uris: vec!["http://localhost".to_string()],
            },
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &json).unwrap();

        let data = std::fs::read_to_string(&path).unwrap();
        let loaded: ClientSecretFile = serde_json::from_str(&data).unwrap();

        assert_eq!(loaded.installed.client_id, "test-id.apps.googleusercontent.com");
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
}

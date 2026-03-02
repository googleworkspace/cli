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

use serde_json::json;

use crate::credential_store;
use crate::error::GwsError;

/// Default scopes for login — broad Workspace access.
pub const DEFAULT_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive",
    "https://www.googleapis.com/auth/spreadsheets",
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/documents",
    "https://www.googleapis.com/auth/presentations",
    "https://www.googleapis.com/auth/tasks",
    "https://www.googleapis.com/auth/pubsub",
    "https://www.googleapis.com/auth/cloud-platform",
];

/// Readonly scopes — read-only Workspace access.
const READONLY_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive.readonly",
    "https://www.googleapis.com/auth/spreadsheets.readonly",
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/calendar.readonly",
    "https://www.googleapis.com/auth/documents.readonly",
    "https://www.googleapis.com/auth/presentations.readonly",
    "https://www.googleapis.com/auth/tasks.readonly",
];

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gws")
}

fn plain_credentials_path() -> PathBuf {
    if let Ok(path) = std::env::var("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE") {
        return PathBuf::from(path);
    }
    config_dir().join("credentials.json")
}

fn token_cache_path() -> PathBuf {
    config_dir().join("token_cache.json")
}

/// Handle `gws auth <subcommand>`.
pub async fn handle_auth_command(args: &[String]) -> Result<(), GwsError> {
    if args.is_empty() {
        return Err(GwsError::Validation(
            "Usage: gws auth <login|setup|status|logout>\n\n\
              login   Authenticate via OAuth2 (opens browser)\n\
                      --readonly   Request read-only scopes\n\
                      --scopes     Comma-separated custom scopes\n\
              setup   Configure GCP project + OAuth client (requires gcloud)\n\
                      --project    Use a specific GCP project\n\
              status  Show current authentication state\n\
              logout  Clear saved credentials and token cache"
                .to_string(),
        ));
    }

    match args[0].as_str() {
        "login" => handle_login(&args[1..]).await,
        "setup" => crate::setup::run_setup(&args[1..]).await,
        "status" => handle_status().await,
        "logout" => handle_logout(),
        other => Err(GwsError::Validation(format!(
            "Unknown auth subcommand: '{other}'. Use: login, setup, status, logout"
        ))),
    }
}
/// Custom delegate that prints the OAuth URL on its own line for easy copying.
struct CliFlowDelegate;

impl yup_oauth2::authenticator_delegate::InstalledFlowDelegate for CliFlowDelegate {
    fn present_user_url<'a>(
        &'a self,
        url: &'a str,
        _need_code: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + 'a>>
    {
        Box::pin(async move {
            eprintln!("Open this URL in your browser to authenticate:\n");
            eprintln!("  {url}\n");
            Ok(String::new())
        })
    }
}

async fn handle_login(args: &[String]) -> Result<(), GwsError> {
    // Resolve client_id and client_secret:
    // 1. Env vars (highest priority)
    // 2. Saved client_secret.json from `gws auth setup` or manual download
    let (client_id, client_secret, project_id) = resolve_client_credentials()?;

    // Persist credentials to client_secret.json if not already saved,
    // so they survive env var removal or shell session changes.
    if !crate::oauth_config::client_config_path().exists() {
        let _ = crate::oauth_config::save_client_config(
            &client_id,
            &client_secret,
            project_id.as_deref().unwrap_or(""),
        );
    }

    // Determine scopes: explicit flags > interactive TUI > defaults
    let scopes = resolve_scopes(args, project_id.as_deref()).await;

    let secret = yup_oauth2::ApplicationSecret {
        client_id: client_id.clone(),
        client_secret: client_secret.clone(),
        auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        redirect_uris: vec!["http://localhost".to_string()],
        ..Default::default()
    };

    // Use a temp file for yup-oauth2's token persistence, then encrypt it
    let temp_path = config_dir().join("credentials.tmp");

    // Always start fresh — delete any stale temp cache from prior login attempts.
    // Without this, yup-oauth2 finds a cached access token and skips the browser flow,
    // which means no refresh_token is returned.
    let _ = std::fs::remove_file(&temp_path);

    // Ensure config directory exists
    if let Some(parent) = temp_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| GwsError::Validation(format!("Failed to create config directory: {e}")))?;
    }

    let auth = yup_oauth2::InstalledFlowAuthenticator::builder(
        secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .persist_tokens_to_disk(&temp_path)
    .force_account_selection(true) // Adds prompt=consent so Google always returns a refresh_token
    .flow_delegate(Box::new(CliFlowDelegate))
    .build()
    .await
    .map_err(|e| GwsError::Auth(format!("Failed to build authenticator: {e}")))?;

    // Request a token — this triggers the browser OAuth flow
    let scope_refs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
    let token = auth
        .token(&scope_refs)
        .await
        .map_err(|e| GwsError::Auth(format!("OAuth flow failed: {e}")))?;

    if token.token().is_some() {
        // Read yup-oauth2's token cache to extract the refresh_token.
        let token_data = std::fs::read_to_string(&temp_path).unwrap_or_default();
        let refresh_token = extract_refresh_token(&token_data).ok_or_else(|| {
            GwsError::Auth(
                "OAuth flow completed but no refresh token was returned. \
                     Ensure the OAuth consent screen includes 'offline' access."
                    .to_string(),
            )
        })?;

        // Build credentials in the standard authorized_user format
        let creds_json = json!({
            "type": "authorized_user",
            "client_id": client_id,
            "client_secret": client_secret,
            "refresh_token": refresh_token,
        });

        let creds_str = serde_json::to_string_pretty(&creds_json)
            .map_err(|e| GwsError::Validation(format!("Failed to serialize credentials: {e}")))?;

        // Save encrypted
        let enc_path = credential_store::save_encrypted(&creds_str)
            .map_err(|e| GwsError::Auth(format!("Failed to encrypt credentials: {e}")))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        let output = json!({
            "status": "success",
            "message": "Authentication successful. Encrypted credentials saved.",
            "credentials_file": enc_path.display().to_string(),
            "encryption": "AES-256-GCM (key derived from hostname + username)",
            "scopes": scopes,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
        Ok(())
    } else {
        // Clean up temp file on failure
        let _ = std::fs::remove_file(&temp_path);
        Err(GwsError::Auth(
            "OAuth flow completed but no token was returned.".to_string(),
        ))
    }
}

/// Resolve OAuth client credentials from env vars or saved config file.
fn resolve_client_credentials() -> Result<(String, String, Option<String>), GwsError> {
    // 1. Try env vars first
    let env_id = std::env::var("GOOGLE_WORKSPACE_CLI_CLIENT_ID").ok();
    let env_secret = std::env::var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET").ok();

    if let (Some(id), Some(secret)) = (env_id, env_secret) {
        // Still try to load project_id from config file for the scope picker
        let project_id = crate::oauth_config::load_client_config()
            .ok()
            .map(|c| c.project_id);
        return Ok((id, secret, project_id));
    }

    // 2. Try saved client_secret.json
    match crate::oauth_config::load_client_config() {
        Ok(config) => Ok((
            config.client_id,
            config.client_secret,
            Some(config.project_id),
        )),
        Err(_) => Err(GwsError::Auth(
            "No OAuth client configured.\n\n\
             Either:\n  \
               1. Run `gws auth setup` to configure a GCP project and OAuth client\n  \
               2. Download client_secret.json from Google Cloud Console and save it to:\n     \
                  ~/.config/gws/client_secret.json\n  \
               3. Set env vars: GOOGLE_WORKSPACE_CLI_CLIENT_ID and GOOGLE_WORKSPACE_CLI_CLIENT_SECRET"
                .to_string(),
        )),
    }
}

/// Resolve OAuth scopes: explicit flags > interactive picker > defaults.
async fn resolve_scopes(args: &[String], project_id: Option<&str>) -> Vec<String> {
    // Explicit --scopes flag takes priority
    for i in 0..args.len() {
        if args[i] == "--scopes" && i + 1 < args.len() {
            return args[i + 1]
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
        }
    }
    if args.iter().any(|a| a == "--readonly") {
        return READONLY_SCOPES.iter().map(|s| s.to_string()).collect();
    }

    // Interactive scope picker when running in a TTY
    if !cfg!(test) && std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        // If we have a project_id, use discovery-based scope picker (rich templates)
        if let Some(pid) = project_id {
            let enabled_apis = crate::setup::get_enabled_apis(pid);
            if !enabled_apis.is_empty() {
                let api_ids: Vec<String> = enabled_apis;
                let scopes = crate::setup::fetch_scopes_for_apis(&api_ids).await;
                if !scopes.is_empty() {
                    if let Some(selected) = run_discovery_scope_picker(&scopes) {
                        return selected;
                    }
                }
            }
        }

        // Fallback: simple scope picker using static SCOPE_ENTRIES
        if let Some(selected) = run_simple_scope_picker() {
            return selected;
        }
    }

    DEFAULT_SCOPES.iter().map(|s| s.to_string()).collect()
}

/// Run the rich discovery-based scope picker with templates.
fn run_discovery_scope_picker(
    relevant_scopes: &[crate::setup::DiscoveredScope],
) -> Option<Vec<String>> {
    use crate::setup::{ScopeClassification, PLATFORM_SCOPE};
    use crate::setup_tui::{PickerResult, SelectItem};

    let mut recommended_scopes = vec![];
    let mut readonly_scopes = vec![];
    let mut all_scopes = vec![];

    // Collect all short names for hierarchical dedup of Full Access template
    let all_shorts: Vec<&str> = relevant_scopes
        .iter()
        .filter(|e| !is_app_only_scope(&e.url))
        .map(|e| e.short.as_str())
        .collect();

    for entry in relevant_scopes {
        // Skip app-only scopes that can't be used with user OAuth
        if is_app_only_scope(&entry.url) {
            continue;
        }

        let is_recommended = if entry.is_readonly {
            let superset = entry.url.strip_suffix(".readonly").unwrap();
            let superset_is_recommended = relevant_scopes
                .iter()
                .any(|s| s.url == superset && s.classification != ScopeClassification::Restricted);
            !superset_is_recommended
        } else {
            entry.classification != ScopeClassification::Restricted
        };

        if is_recommended && !entry.short.starts_with("admin.") {
            recommended_scopes.push(entry.short.to_string());
        }
        if entry.is_readonly {
            readonly_scopes.push(entry.short.to_string());
        }
        // For "Full Access": skip if a broader scope exists (hierarchical dedup)
        // e.g. "drive.metadata" is subsumed by "drive", "calendar.events" by "calendar"
        let is_subsumed = all_shorts.iter().any(|&other| {
            other != entry.short
                && entry.short.starts_with(other)
                && entry.short.as_bytes().get(other.len()) == Some(&b'.')
        });
        if !is_subsumed {
            all_scopes.push(entry.short.to_string());
        }
    }

    let mut items: Vec<SelectItem> = vec![
        SelectItem {
            label: "✨ Recommended (All Non-Restricted + Readonly)".to_string(),
            description: "Selects non-restricted write scopes and all readonly scopes".to_string(),
            selected: true,
            is_fixed: false,
            is_template: true,
            template_selects: recommended_scopes,
        },
        SelectItem {
            label: "🔒 Read Only".to_string(),
            description: "Selects only readonly scopes for enabled APIs".to_string(),
            selected: false,
            is_fixed: false,
            is_template: true,
            template_selects: readonly_scopes,
        },
        SelectItem {
            label: "⚠️ Full Access (All Scopes)".to_string(),
            description: "Selects ALL scopes, including restricted write scopes".to_string(),
            selected: false,
            is_fixed: false,
            is_template: true,
            template_selects: all_scopes,
        },
    ];
    let template_count = items.len();

    let mut valid_scope_indices: Vec<usize> = Vec::new();
    for (idx, entry) in relevant_scopes.iter().enumerate() {
        // Skip app-only scopes from the picker entirely
        if is_app_only_scope(&entry.url) {
            continue;
        }

        let (prefix, emoji) = match entry.classification {
            ScopeClassification::Restricted => ("RESTRICTED ", "⛔ "),
            ScopeClassification::Sensitive => ("SENSITIVE ", "⚠️  "),
            ScopeClassification::NonSensitive => ("", ""),
        };

        let desc_str = if entry.description.is_empty() {
            entry.url.clone()
        } else {
            entry.description.clone()
        };

        let description = if prefix.is_empty() {
            desc_str
        } else {
            format!("{}{}{}", emoji, prefix, desc_str)
        };

        let is_recommended = if entry.is_readonly {
            let superset = entry.url.strip_suffix(".readonly").unwrap();
            let superset_is_recommended = relevant_scopes
                .iter()
                .any(|s| s.url == superset && s.classification != ScopeClassification::Restricted);
            !superset_is_recommended
        } else {
            entry.classification != ScopeClassification::Restricted
        };

        items.push(SelectItem {
            label: entry.short.to_string(),
            description,
            selected: is_recommended,
            is_fixed: false,
            is_template: false,
            template_selects: vec![],
        });
        valid_scope_indices.push(idx);
    }

    match crate::setup_tui::run_picker(
        "Select OAuth scopes",
        "Space to toggle, Enter to confirm",
        items,
        true,
    ) {
        Ok(PickerResult::Confirmed(items)) => {
            let recommended = items.first().is_some_and(|i| i.selected);
            let readonly = items.get(1).is_some_and(|i| i.selected);
            let full = items.get(2).is_some_and(|i| i.selected);

            let mut selected: Vec<String> = Vec::new();

            if full && !recommended && !readonly {
                // Full Access: include all non-app-only scopes
                // (hierarchical dedup is applied in post-processing below)
                for entry in relevant_scopes {
                    if is_app_only_scope(&entry.url) {
                        continue;
                    }
                    selected.push(entry.url.to_string());
                }
            } else if recommended && !full && !readonly {
                // Recommended: non-restricted + readonly, but exclude admin.* scopes
                for entry in relevant_scopes {
                    if is_app_only_scope(&entry.url) {
                        continue;
                    }
                    if entry.short.starts_with("admin.") {
                        continue;
                    }
                    if entry.is_readonly || entry.classification != ScopeClassification::Restricted
                    {
                        selected.push(entry.url.to_string());
                    }
                }
            } else if readonly && !full && !recommended {
                for entry in relevant_scopes {
                    if is_app_only_scope(&entry.url) {
                        continue;
                    }
                    if entry.is_readonly {
                        selected.push(entry.url.to_string());
                    }
                }
            } else {
                for (i, item) in items.iter().enumerate().skip(template_count) {
                    if item.selected {
                        let picker_idx = i - template_count;
                        if let Some(&scope_idx) = valid_scope_indices.get(picker_idx) {
                            if let Some(entry) = relevant_scopes.get(scope_idx) {
                                selected.push(entry.url.to_string());
                            }
                        }
                    }
                }
            }

            // Always include cloud-platform scope
            if !selected.contains(&PLATFORM_SCOPE.to_string()) {
                selected.push(PLATFORM_SCOPE.to_string());
            }

            // Hierarchical dedup: if we have both a broad scope (e.g. `.../auth/drive`)
            // and a narrower scope (e.g. `.../auth/drive.metadata`, `.../auth/drive.readonly`),
            // drop the narrower one since the broad scope subsumes it.
            let prefix = "https://www.googleapis.com/auth/";
            let shorts: Vec<&str> = selected
                .iter()
                .filter_map(|s| s.strip_prefix(prefix))
                .collect();

            let mut deduplicated: Vec<String> = Vec::new();
            for scope in &selected {
                if let Some(short) = scope.strip_prefix(prefix) {
                    // Check if any OTHER selected scope is a prefix of this one
                    // e.g. "drive" is a prefix of "drive.metadata" → drop "drive.metadata"
                    let is_subsumed = shorts.iter().any(|&other| {
                        other != short
                            && short.starts_with(other)
                            && short.as_bytes().get(other.len()) == Some(&b'.')
                    });
                    if is_subsumed {
                        continue;
                    }
                }
                deduplicated.push(scope.clone());
            }

            if deduplicated.len() > 30 {
                eprintln!(
                    "⚠️  Warning: {} scopes selected. Unverified OAuth apps may fail with this many scopes.",
                    deduplicated.len()
                );
            }

            if deduplicated.is_empty() {
                None
            } else {
                Some(deduplicated)
            }
        }
        _ => None, // GoBack, Cancelled, or error
    }
}

/// Run the simple static scope picker (fallback when no project_id available).
fn run_simple_scope_picker() -> Option<Vec<String>> {
    use crate::setup_tui::{PickerResult, SelectItem};

    let items: Vec<SelectItem> = SCOPE_ENTRIES
        .iter()
        .map(|entry| SelectItem {
            label: entry.label.to_string(),
            description: entry.scope.to_string(),
            selected: true,
            is_fixed: false,
            is_template: false,
            template_selects: vec![],
        })
        .collect();

    match crate::setup_tui::run_picker(
        "Select OAuth scopes",
        "Space to toggle, 'a' to select all, Enter to confirm",
        items,
        true,
    ) {
        Ok(PickerResult::Confirmed(items)) => {
            let selected: Vec<String> = items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.selected)
                .map(|(i, _)| SCOPE_ENTRIES[i].scope.to_string())
                .collect();
            if selected.is_empty() {
                None
            } else {
                Some(selected)
            }
        }
        _ => None,
    }
}

async fn handle_status() -> Result<(), GwsError> {
    let plain_path = plain_credentials_path();
    let enc_path = credential_store::encrypted_credentials_path();
    let token_cache = token_cache_path();

    let has_encrypted = enc_path.exists();
    let has_plain = plain_path.exists();
    let has_token_cache = token_cache.exists();

    let auth_method = if has_encrypted || has_plain {
        "oauth2"
    } else {
        "none"
    };

    let storage = if has_encrypted {
        "encrypted"
    } else if has_plain {
        "plaintext"
    } else {
        "none"
    };

    let mut output = json!({
        "auth_method": auth_method,
        "storage": storage,
        "encrypted_credentials": enc_path.display().to_string(),
        "encrypted_credentials_exists": has_encrypted,
        "plain_credentials": plain_path.display().to_string(),
        "plain_credentials_exists": has_plain,
        "token_cache_exists": has_token_cache,
    });

    // Show client config (client_secret.json) status
    let config_path = crate::oauth_config::client_config_path();
    let has_config = config_path.exists();
    output["client_config"] = json!(config_path.display().to_string());
    output["client_config_exists"] = json!(has_config);

    if has_config {
        match crate::oauth_config::load_client_config() {
            Ok(config) => {
                output["project_id"] = json!(config.project_id);
                let masked_id = if config.client_id.len() > 12 {
                    format!(
                        "{}...{}",
                        &config.client_id[..8],
                        &config.client_id[config.client_id.len() - 4..]
                    )
                } else {
                    config.client_id.clone()
                };
                output["config_client_id"] = json!(masked_id);
            }
            Err(e) => {
                output["client_config_error"] = json!(e.to_string());
            }
        }
    }

    // Show credential source by attempting actual resolution
    let has_token_env = std::env::var("GOOGLE_WORKSPACE_CLI_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .is_some();

    let credential_source = if has_token_env {
        output["token_env_var"] = json!(true);
        "token_env_var"
    } else {
        match resolve_client_credentials() {
            Ok((_, _, _)) => {
                let has_env_id = std::env::var("GOOGLE_WORKSPACE_CLI_CLIENT_ID").is_ok();
                let has_env_secret = std::env::var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET").is_ok();
                if has_env_id && has_env_secret {
                    "environment_variables"
                } else {
                    "client_secret.json"
                }
            }
            Err(_) => "none",
        }
    };
    output["credential_source"] = json!(credential_source);

    // Try to read and show masked info from encrypted credentials
    // Skip real credential/network access in test builds
    if !cfg!(test) {
        if has_encrypted {
            match credential_store::load_encrypted() {
                Ok(contents) => {
                    if let Ok(creds) = serde_json::from_str::<serde_json::Value>(&contents) {
                        if let Some(client_id) = creds.get("client_id").and_then(|v| v.as_str()) {
                            let masked = if client_id.len() > 12 {
                                format!(
                                    "{}...{}",
                                    &client_id[..8],
                                    &client_id[client_id.len() - 4..]
                                )
                            } else {
                                client_id.to_string()
                            };
                            output["client_id"] = json!(masked);
                        }
                        output["has_refresh_token"] = json!(creds
                            .get("refresh_token")
                            .and_then(|v| v.as_str())
                            .is_some());
                    }
                    output["encryption_valid"] = json!(true);
                }
                Err(_) => {
                    output["encryption_valid"] = json!(false);
                    output["encryption_error"] =
                        json!("Could not decrypt. May have been created on a different machine.");
                }
            }
        } else if has_plain {
            match tokio::fs::read_to_string(&plain_path).await {
                Ok(contents) => {
                    if let Ok(creds) = serde_json::from_str::<serde_json::Value>(&contents) {
                        if let Some(client_id) = creds.get("client_id").and_then(|v| v.as_str()) {
                            let masked = if client_id.len() > 12 {
                                format!(
                                    "{}...{}",
                                    &client_id[..8],
                                    &client_id[client_id.len() - 4..]
                                )
                            } else {
                                client_id.to_string()
                            };
                            output["client_id"] = json!(masked);
                        }
                        output["has_refresh_token"] = json!(creds.get("refresh_token").is_some());
                    }
                }
                Err(_) => {
                    output["credentials_readable"] = json!(false);
                }
            }
        }
    } // end !cfg!(test)

    // If we have credentials, try to get live info (user, scopes, APIs)
    // Skip all network calls and subprocess spawning in test builds
    if !cfg!(test) {
        let creds_json_str = if has_encrypted {
            credential_store::load_encrypted().ok()
        } else if has_plain {
            tokio::fs::read_to_string(&plain_path).await.ok()
        } else {
            None
        };

        if let Some(creds_str) = creds_json_str {
            if let Ok(creds) = serde_json::from_str::<serde_json::Value>(&creds_str) {
                let client_id = creds.get("client_id").and_then(|v| v.as_str());
                let client_secret = creds.get("client_secret").and_then(|v| v.as_str());
                let refresh_token = creds.get("refresh_token").and_then(|v| v.as_str());

                if let (Some(cid), Some(csec), Some(rt)) = (client_id, client_secret, refresh_token)
                {
                    // Exchange refresh token for access token
                    let http_client = reqwest::Client::new();
                    let token_resp = http_client
                        .post("https://oauth2.googleapis.com/token")
                        .form(&[
                            ("client_id", cid),
                            ("client_secret", csec),
                            ("refresh_token", rt),
                            ("grant_type", "refresh_token"),
                        ])
                        .send()
                        .await;

                    if let Ok(resp) = token_resp {
                        if let Ok(token_json) = resp.json::<serde_json::Value>().await {
                            if let Some(access_token) =
                                token_json.get("access_token").and_then(|v| v.as_str())
                            {
                                output["token_valid"] = json!(true);

                                // Get user info
                                if let Ok(user_resp) = http_client
                                    .get("https://www.googleapis.com/oauth2/v1/userinfo")
                                    .bearer_auth(access_token)
                                    .send()
                                    .await
                                {
                                    if let Ok(user_json) =
                                        user_resp.json::<serde_json::Value>().await
                                    {
                                        if let Some(email) =
                                            user_json.get("email").and_then(|v| v.as_str())
                                        {
                                            output["user"] = json!(email);
                                        }
                                    }
                                }

                                // Get granted scopes via tokeninfo
                                let tokeninfo_url = format!(
                                    "https://oauth2.googleapis.com/tokeninfo?access_token={}",
                                    access_token
                                );
                                if let Ok(info_resp) = http_client.get(&tokeninfo_url).send().await
                                {
                                    if let Ok(info_json) =
                                        info_resp.json::<serde_json::Value>().await
                                    {
                                        if let Some(scope_str) =
                                            info_json.get("scope").and_then(|v| v.as_str())
                                        {
                                            let scopes: Vec<&str> = scope_str.split(' ').collect();
                                            output["scopes"] = json!(scopes);
                                            output["scope_count"] = json!(scopes.len());
                                        }
                                    }
                                }
                            } else {
                                output["token_valid"] = json!(false);
                                if let Some(err) =
                                    token_json.get("error_description").and_then(|v| v.as_str())
                                {
                                    output["token_error"] = json!(err);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Show enabled APIs if we have a project_id
        if let Some(pid) = output.get("project_id").and_then(|v| v.as_str()) {
            let enabled = crate::setup::get_enabled_apis(pid);
            if !enabled.is_empty() {
                output["enabled_apis"] = json!(enabled);
                output["enabled_api_count"] = json!(enabled.len());
            }
        }
    } // end !cfg!(test)

    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_default()
    );
    Ok(())
}

fn handle_logout() -> Result<(), GwsError> {
    let plain_path = plain_credentials_path();
    let enc_path = credential_store::encrypted_credentials_path();
    let token_cache = token_cache_path();

    let mut removed = Vec::new();

    for path in [&enc_path, &plain_path, &token_cache] {
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| {
                GwsError::Validation(format!("Failed to remove {}: {e}", path.display()))
            })?;
            removed.push(path.display().to_string());
        }
    }

    let output = if removed.is_empty() {
        json!({
            "status": "success",
            "message": "No credentials found to remove.",
        })
    } else {
        json!({
            "status": "success",
            "message": "Logged out. Credentials and token cache removed.",
            "removed": removed,
        })
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_default()
    );
    Ok(())
}

/// Extract refresh_token from yup-oauth2 v12 token cache.
/// Format: [{"scopes":[...], "token":{"access_token":..., "refresh_token":...}}]
pub fn extract_refresh_token(token_data: &str) -> Option<String> {
    let cache: serde_json::Value = serde_json::from_str(token_data).ok()?;
    cache.as_array()?.iter().find_map(|entry| {
        entry
            .get("token")
            .and_then(|t| t.get("refresh_token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    })
}

/// Parse --scopes or --readonly from args, falling back to DEFAULT_SCOPES.
/// Scope entry with a human-readable label for the TUI picker.
struct ScopeEntry {
    scope: &'static str,
    label: &'static str,
}

const SCOPE_ENTRIES: &[ScopeEntry] = &[
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/drive",
        label: "Google Drive",
    },
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/spreadsheets",
        label: "Google Sheets",
    },
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/gmail.modify",
        label: "Gmail",
    },
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/calendar",
        label: "Google Calendar",
    },
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/documents",
        label: "Google Docs",
    },
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/presentations",
        label: "Google Slides",
    },
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/tasks",
        label: "Google Tasks",
    },
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/pubsub",
        label: "Cloud Pub/Sub",
    },
    ScopeEntry {
        scope: "https://www.googleapis.com/auth/cloud-platform",
        label: "Cloud Platform",
    },
];

// (parse_scopes removed — replaced by resolve_scopes above)

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper to run resolve_scopes in tests (async).
    fn run_resolve_scopes(args: &[String], project_id: Option<&str>) -> Vec<String> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(resolve_scopes(args, project_id))
    }

    #[test]
    fn resolve_scopes_returns_defaults_when_no_flag() {
        let args: Vec<String> = vec![];
        let scopes = run_resolve_scopes(&args, None);
        assert_eq!(scopes.len(), DEFAULT_SCOPES.len());
        assert_eq!(scopes[0], "https://www.googleapis.com/auth/drive");
    }

    #[test]
    fn resolve_scopes_returns_custom_scopes() {
        let args: Vec<String> = vec![
            "--scopes".to_string(),
            "https://www.googleapis.com/auth/drive.readonly".to_string(),
        ];
        let scopes = run_resolve_scopes(&args, None);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0], "https://www.googleapis.com/auth/drive.readonly");
    }

    #[test]
    fn resolve_scopes_handles_multiple_comma_separated() {
        let args: Vec<String> = vec![
            "--scopes".to_string(),
            "https://www.googleapis.com/auth/drive, https://www.googleapis.com/auth/gmail.readonly"
                .to_string(),
        ];
        let scopes = run_resolve_scopes(&args, None);
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0], "https://www.googleapis.com/auth/drive");
        assert_eq!(scopes[1], "https://www.googleapis.com/auth/gmail.readonly");
    }

    #[test]
    fn resolve_scopes_ignores_trailing_flag() {
        // --scopes with no value should use defaults
        let args: Vec<String> = vec!["--scopes".to_string()];
        let scopes = run_resolve_scopes(&args, None);
        assert_eq!(scopes.len(), DEFAULT_SCOPES.len());
    }

    #[test]
    fn resolve_scopes_readonly_returns_readonly_scopes() {
        let args = vec!["--readonly".to_string()];
        let scopes = run_resolve_scopes(&args, None);
        assert_eq!(scopes.len(), READONLY_SCOPES.len());
        for scope in &scopes {
            assert!(
                scope.ends_with(".readonly"),
                "Expected readonly scope, got: {scope}"
            );
        }
    }

    #[test]
    fn resolve_scopes_custom_overrides_readonly() {
        // --scopes takes priority over --readonly
        let args = vec![
            "--scopes".to_string(),
            "https://www.googleapis.com/auth/drive".to_string(),
            "--readonly".to_string(),
        ];
        let scopes = run_resolve_scopes(&args, None);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0], "https://www.googleapis.com/auth/drive");
    }

    #[test]
    fn resolve_client_credentials_from_env_vars() {
        let _lock = TEST_MUTEX.lock().unwrap();
        std::env::set_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID", "test-id");
        std::env::set_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET", "test-secret");
        let result = resolve_client_credentials();
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID");
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET");
        let (id, secret, _project_id) = result.unwrap();
        assert_eq!(id, "test-id");
        assert_eq!(secret, "test-secret");
        // project_id may be Some if client_secret.json exists on the machine
    }

    #[test]
    fn resolve_client_credentials_missing_env_vars_uses_config() {
        let _lock = TEST_MUTEX.lock().unwrap();
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID");
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET");
        // Result depends on whether client_secret.json exists on the machine
        let result = resolve_client_credentials();
        if crate::oauth_config::client_config_path().exists() {
            assert!(
                result.is_ok(),
                "Should succeed when client_secret.json exists"
            );
        } else {
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("No OAuth client configured"));
        }
    }

    #[test]
    fn config_dir_returns_gws_subdir() {
        let path = config_dir();
        assert!(path.ends_with("gws"));
    }

    #[test]
    fn plain_credentials_path_defaults_to_config_dir() {
        let _lock = TEST_MUTEX.lock().unwrap();
        // Without env var, should be in config dir
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE");
        let path = plain_credentials_path();
        assert!(path.ends_with("credentials.json"));
        assert!(path.starts_with(config_dir()));
    }

    #[test]
    fn plain_credentials_path_respects_env_var() {
        let _lock = TEST_MUTEX.lock().unwrap();
        std::env::set_var(
            "GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE",
            "/tmp/test-creds.json",
        );
        let path = plain_credentials_path();
        assert_eq!(path, PathBuf::from("/tmp/test-creds.json"));
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE");
    }

    #[test]
    fn token_cache_path_is_in_config_dir() {
        let path = token_cache_path();
        assert!(path.ends_with("token_cache.json"));
        assert!(path.starts_with(config_dir()));
    }

    #[tokio::test]
    async fn handle_auth_command_empty_args_returns_usage() {
        let args: Vec<String> = vec![];
        let result = handle_auth_command(&args).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            GwsError::Validation(msg) => assert!(msg.contains("login|setup|status|logout")),
            other => panic!("Expected Validation error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_auth_command_invalid_subcommand() {
        let args = vec!["frobnicate".to_string()];
        let result = handle_auth_command(&args).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            GwsError::Validation(msg) => assert!(msg.contains("frobnicate")),
            other => panic!("Expected Validation error, got: {other:?}"),
        }
    }

    #[test]
    fn resolve_credentials_fails_without_env_vars_or_config() {
        let _lock = TEST_MUTEX.lock().unwrap();
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID");
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET");
        // When no env vars AND no client_secret.json on disk, should fail
        let result = resolve_client_credentials();
        if !crate::oauth_config::client_config_path().exists() {
            assert!(result.is_err());
            match result.unwrap_err() {
                GwsError::Auth(msg) => assert!(msg.contains("No OAuth client configured")),
                other => panic!("Expected Auth error, got: {other:?}"),
            }
        }
        // If client_secret.json exists on the dev machine, credentials resolve
        // successfully — that's correct behavior, not a test failure.
    }

    #[test]
    fn resolve_credentials_uses_env_vars_when_present() {
        let _lock = TEST_MUTEX.lock().unwrap();
        std::env::set_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID", "test-id");
        std::env::set_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET", "test-secret");

        let result = resolve_client_credentials();

        // Clean up immediately
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID");
        std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET");

        let (id, secret, _) = result.unwrap();
        assert_eq!(id, "test-id");
        assert_eq!(secret, "test-secret");
    }

    #[tokio::test]
    async fn handle_status_succeeds_without_credentials() {
        // status should always succeed and report "none"
        let args = vec!["status".to_string()];
        let result = handle_auth_command(&args).await;
        assert!(result.is_ok());
    }

    #[test]
    fn credential_store_save_load_round_trip() {
        // Use encrypt/decrypt directly to avoid writing to the real config dir
        let json = r#"{"client_id":"test","client_secret":"secret","refresh_token":"tok"}"#;
        let encrypted = credential_store::encrypt(json.as_bytes()).expect("encrypt should succeed");
        let decrypted = credential_store::decrypt(&encrypted).expect("decrypt should succeed");
        assert_eq!(String::from_utf8(decrypted).unwrap(), json);
    }

    #[test]
    fn extract_refresh_token_from_yup_oauth2_format() {
        // Actual format produced by yup-oauth2 v12
        let data = r#"[{"scopes":["https://www.googleapis.com/auth/drive"],"token":{"access_token":"ya29.test","refresh_token":"1//test-refresh-token","expires_at":[2026,43,19,44,15,0,0,0,0],"id_token":null}}]"#;
        assert_eq!(
            extract_refresh_token(data),
            Some("1//test-refresh-token".to_string())
        );
    }

    #[test]
    fn extract_refresh_token_missing_token() {
        let data = r#"[{"scopes":["scope"],"token":{"access_token":"ya29.test"}}]"#;
        assert_eq!(extract_refresh_token(data), None);
    }

    #[test]
    fn extract_refresh_token_empty_array() {
        assert_eq!(extract_refresh_token("[]"), None);
    }

    #[test]
    fn extract_refresh_token_invalid_json() {
        assert_eq!(extract_refresh_token("not json"), None);
    }

    #[test]
    fn extract_refresh_token_object_format() {
        // Should return None — we only support array format
        let data = r#"{"key":{"access_token":"ya29","refresh_token":"1//tok"}}"#;
        assert_eq!(extract_refresh_token(data), None);
    }
}

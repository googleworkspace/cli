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

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::json;

use crate::credential_store;
use crate::error::GwsError;

/// Mask a secret string by showing only the first 4 and last 4 characters.
/// Strings with 8 or fewer characters are fully replaced with "***".
fn mask_secret(s: &str) -> String {
    const MASK_PREFIX_LEN: usize = 4;
    const MASK_SUFFIX_LEN: usize = 4;
    const MIN_LEN_FOR_PARTIAL_MASK: usize = MASK_PREFIX_LEN + MASK_SUFFIX_LEN;

    if s.len() > MIN_LEN_FOR_PARTIAL_MASK {
        format!(
            "{}...{}",
            &s[..MASK_PREFIX_LEN],
            &s[s.len() - MASK_SUFFIX_LEN..]
        )
    } else {
        "***".to_string()
    }
}

/// Minimal scopes for first-run login — only core Workspace APIs that never
/// trigger Google's `restricted_client` / unverified-app block.
///
/// These are the safest scopes for unverified OAuth apps and personal Cloud
/// projects.  Users can request broader access with `--scopes` or `--full`.
pub const MINIMAL_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive",
    "https://www.googleapis.com/auth/spreadsheets",
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/documents",
    "https://www.googleapis.com/auth/presentations",
    "https://www.googleapis.com/auth/tasks",
];

/// Default scopes for login.  Alias for [`MINIMAL_SCOPES`] — deliberately kept
/// narrow so first-run logins succeed even with an unverified OAuth app.
///
/// Previously this included `pubsub` and `cloud-platform`, which Google marks
/// as *restricted* and blocks for unverified apps, causing `Error 403:
/// restricted_client`.  Use `--scopes` to add those scopes explicitly when you
/// have a verified app or a GCP project with the APIs enabled and approved.
pub const DEFAULT_SCOPES: &[&str] = MINIMAL_SCOPES;

/// Full scopes — all common Workspace APIs plus GCP platform access.
///
/// Use `gws auth login --full` to request these.  Unverified OAuth apps will
/// receive a Google consent-screen warning, and some scopes (e.g. `pubsub`,
/// `cloud-platform`) require app verification or a Workspace domain admin to
/// grant access.
pub const FULL_SCOPES: &[&str] = &[
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

pub fn base_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("GOOGLE_WORKSPACE_CLI_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        // Use ~/.config/gws on all platforms for a consistent, user-friendly path.
        let primary = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("gws");
        
        if primary.exists() {
            primary
        } else {
            // Backward compat: fall back to OS-specific config dir for existing installs
            // (e.g. ~/Library/Application Support/gws on macOS, %APPDATA%\gws on Windows).
            let legacy = dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("gws");
            if legacy.exists() {
                legacy
            } else {
                primary
            }
        }
    }
}

pub fn get_active_profile() -> Option<String> {
    std::env::var("GOOGLE_WORKSPACE_CLI_PROFILE")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let base_dir = base_config_dir();
            std::fs::read_to_string(base_dir.join("active_profile"))
                .ok()
                .and_then(|s| {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    match validate_profile_name(trimmed) {
                        Ok(_) => Some(trimmed.to_string()),
                        Err(e) => {
                            eprintln!(
                                "Error: Invalid profile name '{}' found in active_profile file: {}. Please fix it or remove the file. Defaulting to 'default' profile.",
                                trimmed, e
                            );
                            None
                        }
                    }
                })
        })
}

pub fn validate_profile_name(profile_name: &str) -> Result<(), GwsError> {
    if profile_name.is_empty()
        || profile_name == "."
        || profile_name == ".."
        || profile_name.chars().any(|c| !(c.is_alphanumeric() || c == '-' || c == '_'))
    {
        return Err(GwsError::Validation(
            "Invalid profile name. It must not be empty, '.', or '..', and can only contain alphanumeric characters, dashes (-), and underscores (_).".to_string(),
        ));
    }
    Ok(())
}

pub fn config_dir() -> PathBuf {
    let base_dir = base_config_dir();

    match get_active_profile().as_deref() {
        Some("default") | None => base_dir,
        Some(name) => base_dir.join("profiles").join(name),
    }
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
    const USAGE: &str = concat!(
        "Usage: gws auth <login|setup|status|switch|export|logout> [options]\n\n",
        "  login    Authenticate via OAuth2 (opens browser)\n",
        "           --readonly       Request read-only scopes\n",
        "           --full           Request all scopes incl. pubsub + cloud-platform\n",
        "                            (may trigger restricted_client for unverified apps)\n",
        "           --scopes         Comma-separated custom scopes\n",
        "           -s, --services   Comma-separated service names to limit scope picker\n",
        "                            (e.g. -s drive,gmail,sheets)\n",
        "  setup    Configure GCP project + OAuth client (requires gcloud)\n",
        "           --project        Use a specific GCP project\n",
        "  status   Show current authentication state\n",
        "  switch   <profile> Switch the active configuration profile\n",
        "  export   Print decrypted credentials to stdout\n",
        "  logout   Clear saved credentials and token cache",
    );

    // Honour --help / -h before treating the first arg as a subcommand.
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("{USAGE}");
        return Ok(());
    }

    match args[0].as_str() {
        "login" => handle_login(&args[1..]).await,
        "setup" => crate::setup::run_setup(&args[1..]).await,
        "status" => handle_status().await,
        "switch" => handle_switch(&args[1..]),
        "export" => {
            let unmasked = args.len() > 1 && args[1] == "--unmasked";
            handle_export(unmasked).await
        }
        "logout" => handle_logout(),
        other => Err(GwsError::Validation(format!(
            "Unknown auth subcommand: '{other}'. Use: login, setup, status, switch, export, logout"
        ))),
    }
}

fn handle_switch(args: &[String]) -> Result<(), GwsError> {
    if args.is_empty() {
        return Err(GwsError::Validation(
            "Missing profile name. Usage: gws auth switch <profile_name>".to_string(),
        ));
    }

    let profile_name = &args[0];

    validate_profile_name(profile_name)?;

    // Read the base directory without applying the current active profile
    let base_dir = base_config_dir();

    if !base_dir.exists() {
        std::fs::create_dir_all(&base_dir).map_err(|e| {
            GwsError::Validation(format!("Failed to create base config dir: {e}"))
        })?;
    }

    let active_profile_path = base_dir.join("active_profile");

    if profile_name == "default" {
        if let Err(e) = std::fs::remove_file(active_profile_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(GwsError::Validation(format!("Failed to remove active_profile file: {e}")));
            }
        }
        println!("Switched to default profile.");
    } else {
        std::fs::write(&active_profile_path, profile_name).map_err(|e| {
            GwsError::Validation(format!("Failed to write active_profile: {e}"))
        })?;
        println!("Switched to profile: {profile_name}");
    }

    Ok(())
}

/// Custom delegate that prints the OAuth URL on its own line for easy copying.
/// Optionally includes `login_hint` in the URL for account pre-selection.
struct CliFlowDelegate {
    login_hint: Option<String>,
}

impl yup_oauth2::authenticator_delegate::InstalledFlowDelegate for CliFlowDelegate {
    fn present_user_url<'a>(
        &'a self,
        url: &'a str,
        _need_code: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + 'a>>
    {
        Box::pin(async move {
            // Inject login_hint into the OAuth URL if we have one
            let display_url = if let Some(ref hint) = self.login_hint {
                let encoded: String = percent_encoding::percent_encode(
                    hint.as_bytes(),
                    percent_encoding::NON_ALPHANUMERIC,
                )
                .to_string();
                if url.contains('?') {
                    format!("{url}&login_hint={encoded}")
                } else {
                    format!("{url}?login_hint={encoded}")
                }
            } else {
                url.to_string()
            };
            eprintln!("Open this URL in your browser to authenticate:\n");
            eprintln!("  {display_url}\n");
            Ok(String::new())
        })
    }
}

async fn handle_login(args: &[String]) -> Result<(), GwsError> {
    // Extract -s/--services from args
    let mut services_filter: Option<HashSet<String>> = None;
    let mut filtered_args: Vec<String> = Vec::new();
    let mut skip_next = false;
    for i in 0..args.len() {
        if skip_next {
            skip_next = false;
            continue;
        }
        let services_str = if (args[i] == "-s" || args[i] == "--services") && i + 1 < args.len() {
            skip_next = true;
            Some(args[i + 1].as_str())
        } else {
            args[i].strip_prefix("--services=")
        };

        if let Some(value) = services_str {
            services_filter = Some(
                value
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect(),
            );
            continue;
        }
        filtered_args.push(args[i].clone());
    }

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
    let scopes = resolve_scopes(
        &filtered_args,
        project_id.as_deref(),
        services_filter.as_ref(),
    )
    .await;

    // Remove restrictive scopes when broader alternatives are present.
    let mut scopes = filter_redundant_restrictive_scopes(scopes);

    let secret = yup_oauth2::ApplicationSecret {
        client_id: client_id.clone(),
        client_secret: client_secret.clone(),
        auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        redirect_uris: vec!["http://localhost".to_string()],
        ..Default::default()
    };

    // Ensure openid + email scopes are always present so we can identify the user
    // via the userinfo endpoint after login.
    let identity_scopes = ["openid", "https://www.googleapis.com/auth/userinfo.email"];
    for s in &identity_scopes {
        if !scopes.iter().any(|existing| existing == s) {
            scopes.push(s.to_string());
        }
    }

    // Use a temp file for yup-oauth2's token persistence, then encrypt it
    let temp_path = config_dir().join("credentials.tmp");

    // Always start fresh — delete any stale temp cache from prior login attempts.
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
    .with_storage(Box::new(crate::token_storage::EncryptedTokenStorage::new(
        temp_path.clone(),
    )))
    .force_account_selection(true) // Adds prompt=consent so Google always returns a refresh_token
    .flow_delegate(Box::new(CliFlowDelegate { login_hint: None }))
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
        // EncryptedTokenStorage stores data encrypted, so we must decrypt first.
        let token_data = std::fs::read(&temp_path)
            .ok()
            .and_then(|bytes| crate::credential_store::decrypt(&bytes).ok())
            .and_then(|decrypted| String::from_utf8(decrypted).ok())
            .unwrap_or_default();
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

        // Fetch the user's email from Google userinfo
        let access_token = token.token().unwrap_or_default();
        let actual_email = fetch_userinfo_email(access_token).await;

        // Save encrypted credentials
        let enc_path = credential_store::save_encrypted(&creds_str)
            .map_err(|e| GwsError::Auth(format!("Failed to encrypt credentials: {e}")))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        let output = json!({
            "status": "success",
            "message": "Authentication successful. Encrypted credentials saved.",
            "account": actual_email.as_deref().unwrap_or("(unknown)"),
            "credentials_file": enc_path.display().to_string(),
            "encryption": "AES-256-GCM (key secured by OS Keyring or local `.encryption_key`)",
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

/// Fetch the authenticated user's email from Google's userinfo endpoint.
async fn fetch_userinfo_email(access_token: &str) -> Option<String> {
    let client = match crate::client::build_client() {
        Ok(c) => c,
        Err(_) => return None,
    };
    let resp = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    body.get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

async fn handle_export(unmasked: bool) -> Result<(), GwsError> {
    let enc_path = credential_store::encrypted_credentials_path();
    if !enc_path.exists() {
        return Err(GwsError::Auth(
            "No encrypted credentials found. Run 'gws auth login' first.".to_string(),
        ));
    }

    match credential_store::load_encrypted() {
        Ok(contents) => {
            if unmasked {
                println!("{contents}");
            } else if let Ok(mut creds) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(obj) = creds.as_object_mut() {
                    for key in ["client_secret", "refresh_token"] {
                        if let Some(val) = obj.get_mut(key) {
                            if let Some(s) = val.as_str() {
                                *val = json!(mask_secret(s));
                            }
                        }
                    }
                }
                println!("{}", serde_json::to_string_pretty(&creds).unwrap());
            } else {
                println!("{contents}");
            }
            Ok(())
        }
        Err(e) => Err(GwsError::Auth(format!(
            "Failed to decrypt credentials: {e}. May have been created on a different machine.",
        ))),
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
            format!(
                "No OAuth client configured.\n\n\
                 Either:\n  \
                   1. Run `gws auth setup` to configure a GCP project and OAuth client\n  \
                   2. Download client_secret.json from Google Cloud Console and save it to:\n     \
                      {}\n  \
                   3. Set env vars: GOOGLE_WORKSPACE_CLI_CLIENT_ID and GOOGLE_WORKSPACE_CLI_CLIENT_SECRET",
                crate::oauth_config::client_config_path().display()
            ),
        )),
    }
}

/// Resolve OAuth scopes: explicit flags > interactive picker > defaults.
///
/// When `services_filter` is `Some`, only scopes belonging to the specified
/// services are shown in the picker (or returned in non-interactive mode).
async fn resolve_scopes(
    args: &[String],
    project_id: Option<&str>,
    services_filter: Option<&HashSet<String>>,
) -> Vec<String> {
    // Explicit --scopes flag takes priority (bypasses services filter)
    for i in 0..args.len() {
        if args[i] == "--scopes" && i + 1 < args.len() {
            return args[i + 1]
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
        }
    }
    if args.iter().any(|a| a == "--readonly") {
        let scopes: Vec<String> = READONLY_SCOPES.iter().map(|s| s.to_string()).collect();
        return filter_scopes_by_services(scopes, services_filter);
    }
    if args.iter().any(|a| a == "--full") {
        let scopes: Vec<String> = FULL_SCOPES.iter().map(|s| s.to_string()).collect();
        return filter_scopes_by_services(scopes, services_filter);
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
                    if let Some(selected) = run_discovery_scope_picker(&scopes, services_filter) {
                        return selected;
                    }
                }
            }
        }

        // Fallback: simple scope picker using static SCOPE_ENTRIES
        if let Some(selected) = run_simple_scope_picker(services_filter) {
            return selected;
        }
    }

    let defaults: Vec<String> = DEFAULT_SCOPES.iter().map(|s| s.to_string()).collect();
    filter_scopes_by_services(defaults, services_filter)
}

/// Check if a scope URL belongs to one of the specified services.
///
/// Matching is done on the scope's short name (the part after
/// `https://www.googleapis.com/auth/`). A scope matches a service if its
/// short name equals the service or starts with `service.` (e.g. service
/// `drive` matches `drive`, `drive.readonly`, `drive.metadata.readonly`).
///
/// The `cloud-platform` scope always passes through since it's a
/// cross-service platform scope.
fn scope_matches_service(scope_url: &str, services: &HashSet<String>) -> bool {
    let short = scope_url
        .strip_prefix("https://www.googleapis.com/auth/")
        .unwrap_or(scope_url);

    // cloud-platform is a cross-service scope, always include
    if short == "cloud-platform" {
        return true;
    }

    let prefix = short.split('.').next().unwrap_or(short);

    services.iter().any(|svc| {
        // Map common user-friendly service names to their OAuth scope prefixes
        let mapped_svc = match svc.as_str() {
            "sheets" => "spreadsheets",
            "slides" => "presentations",
            "docs" => "documents",
            s => s,
        };
        prefix == mapped_svc || short.starts_with(&format!("{mapped_svc}."))
    })
}

/// Remove restrictive scopes that are redundant when broader alternatives
/// are present. For example, `gmail.metadata` restricts query parameters
/// and is unnecessary when `gmail.modify`, `gmail.readonly`, or the full
/// `https://mail.google.com/` scope is already included.
///
/// This prevents Google from enforcing the restrictive scope's limitations
/// on the access token even though broader access was granted.
fn filter_redundant_restrictive_scopes(scopes: Vec<String>) -> Vec<String> {
    // Scopes that restrict API behavior when present in a token, even alongside
    // broader scopes. Each entry maps a restrictive scope to the broader scopes
    // that make it redundant. The restrictive scope is removed only if at least
    // one of its broader alternatives is already in the list.
    const RESTRICTIVE_SCOPES: &[(&str, &[&str])] = &[(
        "https://www.googleapis.com/auth/gmail.metadata",
        &[
            "https://mail.google.com/",
            "https://www.googleapis.com/auth/gmail.modify",
            "https://www.googleapis.com/auth/gmail.readonly",
        ],
    )];

    let scope_set: std::collections::HashSet<String> = scopes.iter().cloned().collect();

    scopes
        .into_iter()
        .filter(|scope| {
            !RESTRICTIVE_SCOPES.iter().any(|(restrictive, broader)| {
                scope.as_str() == *restrictive && broader.iter().any(|b| scope_set.contains(*b))
            })
        })
        .collect()
}

/// Filter a list of scope URLs to only those matching the given services.
/// If no filter is provided, returns all scopes unchanged.
fn filter_scopes_by_services(
    scopes: Vec<String>,
    services_filter: Option<&HashSet<String>>,
) -> Vec<String> {
    match services_filter {
        Some(services) if !services.is_empty() => scopes
            .into_iter()
            .filter(|s| scope_matches_service(s, services))
            .collect(),
        _ => scopes,
    }
}

/// Check if a scope is subsumed by a broader scope in the list.
/// e.g. "drive.metadata" is subsumed by "drive", "calendar.events" by "calendar".
fn is_subsumed_scope(short: &str, all_shorts: &[&str]) -> bool {
    all_shorts.iter().any(|&other| {
        other != short
            && short.starts_with(other)
            && short.as_bytes().get(other.len()) == Some(&b'.')
    })
}

/// Determine if a discovered scope should be included in the "Recommended" template.
///
/// When a services filter is active, recommends all top-level (non-subsumed) scopes.
/// Otherwise, recommends only the curated `MINIMAL_SCOPES` list to stay under
/// the 25-scope limit for unverified apps and @gmail.com accounts.
///
/// Always excludes admin-only and Workspace-admin scopes.
fn is_recommended_scope(
    entry: &crate::setup::DiscoveredScope,
    all_shorts: &[&str],
    has_services_filter: bool,
) -> bool {
    if entry.short.starts_with("admin.") || is_workspace_admin_scope(&entry.url) {
        return false;
    }
    if has_services_filter {
        !is_subsumed_scope(&entry.short, all_shorts)
    } else {
        MINIMAL_SCOPES.contains(&entry.url.as_str())
    }
}

/// Run the rich discovery-based scope picker with templates.
fn run_discovery_scope_picker(
    relevant_scopes: &[crate::setup::DiscoveredScope],
    services_filter: Option<&HashSet<String>>,
) -> Option<Vec<String>> {
    use crate::setup::{ScopeClassification, PLATFORM_SCOPE};
    use crate::setup_tui::{PickerResult, SelectItem};

    let mut recommended_scopes = vec![];
    let mut readonly_scopes = vec![];
    let mut all_scopes = vec![];

    // Pre-filter scopes by services if a filter is specified
    let filtered_scopes: Vec<&crate::setup::DiscoveredScope> = relevant_scopes
        .iter()
        .filter(|e| {
            services_filter.is_none_or(|services| {
                services.is_empty() || scope_matches_service(&e.url, services)
            })
        })
        .collect();

    // Collect all short names for hierarchical dedup of Full Access template
    let all_shorts: Vec<&str> = filtered_scopes
        .iter()
        .filter(|e| !is_app_only_scope(&e.url))
        .map(|e| e.short.as_str())
        .collect();

    for entry in &filtered_scopes {
        // Skip app-only scopes that can't be used with user OAuth
        if is_app_only_scope(&entry.url) {
            continue;
        }

        if is_recommended_scope(entry, &all_shorts, services_filter.is_some()) {
            recommended_scopes.push(entry.short.to_string());
        }
        if entry.is_readonly {
            readonly_scopes.push(entry.short.to_string());
        }
        // For "Full Access": skip if a broader scope exists (hierarchical dedup)
        // e.g. "drive.metadata" is subsumed by "drive", "calendar.events" by "calendar"
        if !is_subsumed_scope(&entry.short, &all_shorts) {
            all_scopes.push(entry.short.to_string());
        }
    }

    let mut items: Vec<SelectItem> = vec![
        SelectItem {
            label: "✨ Recommended (Core Consumer Scopes)".to_string(),
            description: "Selects Drive, Gmail, Calendar, Docs, Sheets, Slides, and Tasks"
                .to_string(),
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
    for (idx, entry) in filtered_scopes.iter().enumerate() {
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
            let superset = entry.url.strip_suffix(".readonly").unwrap_or(&entry.url);
            let superset_is_recommended = filtered_scopes
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
                for entry in &filtered_scopes {
                    if is_app_only_scope(&entry.url) {
                        continue;
                    }
                    selected.push(entry.url.to_string());
                }
            } else if recommended && !full && !readonly {
                // Recommended: consumer scopes only (or top-level scopes if filtered).
                for entry in &filtered_scopes {
                    if is_app_only_scope(&entry.url) {
                        continue;
                    }
                    if is_recommended_scope(entry, &all_shorts, services_filter.is_some()) {
                        selected.push(entry.url.to_string());
                    }
                }
            } else if readonly && !full && !recommended {
                for entry in &filtered_scopes {
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
                            if let Some(entry) = filtered_scopes.get(scope_idx) {
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
fn run_simple_scope_picker(services_filter: Option<&HashSet<String>>) -> Option<Vec<String>> {
    use crate::setup_tui::{PickerResult, SelectItem};

    // Pre-filter SCOPE_ENTRIES by services if a filter is provided
    let entries: Vec<&ScopeEntry> = SCOPE_ENTRIES
        .iter()
        .filter(|entry| {
            services_filter.is_none_or(|services| {
                services.is_empty() || scope_matches_service(entry.scope, services)
            })
        })
        .collect();

    let items: Vec<SelectItem> = entries
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
                .map(|(i, _)| entries[i].scope.to_string())
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
    // Determine the active profile
    let profile = get_active_profile().unwrap_or_else(|| "default".to_string());
        
    println!("\nActive Profile: {profile}");

    Ok(())
}

fn handle_logout() -> Result<(), GwsError> {
    let plain_path = plain_credentials_path();
    let enc_path = credential_store::encrypted_credentials_path();
    let token_cache = token_cache_path();
    let sa_token_cache = config_dir().join("sa_token_cache.json");

    let mut removed = Vec::new();

    for path in [&enc_path, &plain_path, &token_cache, &sa_token_cache] {
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
            "message": "Logged out. All credentials and token caches removed.",
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
///
/// Supports two formats:
/// 1. Array format (yup-oauth2 default file storage):
///    [{"scopes":[...], "token":{"access_token":..., "refresh_token":...}}]
/// 2. Object/HashMap format (EncryptedTokenStorage serialization):
///    {"scope_key": {"access_token":..., "refresh_token":..., ...}}
pub fn extract_refresh_token(token_data: &str) -> Option<String> {
    let cache: serde_json::Value = serde_json::from_str(token_data).ok()?;

    // Format 1: array of {scopes, token} entries
    if let Some(arr) = cache.as_array() {
        let result = arr.iter().find_map(|entry| {
            entry
                .get("token")
                .and_then(|t| t.get("refresh_token"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
        if result.is_some() {
            return result;
        }
    }

    // Format 2: HashMap<String, TokenInfo> — values are TokenInfo structs
    if let Some(obj) = cache.as_object() {
        for value in obj.values() {
            if let Some(rt) = value.get("refresh_token").and_then(|v| v.as_str()) {
                return Some(rt.to_string());
            }
        }
    }

    None
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

/// Helper: check if a scope can't be used with user OAuth consent flow
/// (requires a Chat app or service account).
fn is_app_only_scope(url: &str) -> bool {
    url.contains("/auth/chat.app.")
        || url.contains("/auth/chat.bot")
        || url.contains("/auth/chat.import")
        || url.contains("/auth/keep")
        || url.contains("/auth/apps.alerts")
}

/// Helper: check if a scope requires Workspace domain admin access and therefore
/// cannot be granted to personal `@gmail.com` accounts via standard user OAuth.
///
/// These scopes are valid in Workspace environments with a domain admin, but
/// Google returns `400 invalid_scope` when requested by personal accounts.
/// They are excluded from the "Recommended" preset to avoid login failures.
///
/// Affected scope families:
/// - `apps.*`            — Alert Center, Groups Settings, Licensing, Reseller
/// - `cloud-identity.*`  — Cloud Identity: devices, groups, inbound SSO, policies
/// - `ediscovery`        — Google Vault
/// - `directory.readonly`— Admin SDK Directory (read-only)
/// - `groups`            — Groups Management
fn is_workspace_admin_scope(url: &str) -> bool {
    let short = url
        .strip_prefix("https://www.googleapis.com/auth/")
        .unwrap_or(url);
    short.starts_with("apps.")
        || short.starts_with("cloud-identity.")
        || short.starts_with("chat.admin.")
        || short.starts_with("classroom.")
        || short == "ediscovery"
        || short == "directory.readonly"
        || short == "groups"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to run resolve_scopes in tests (async).
    fn run_resolve_scopes(args: &[String], project_id: Option<&str>) -> Vec<String> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(resolve_scopes(args, project_id, None))
    }

    /// Helper to run resolve_scopes with a services filter.
    fn run_resolve_scopes_with_services(
        args: &[String],
        project_id: Option<&str>,
        services: &[&str],
    ) -> Vec<String> {
        let filter: HashSet<String> = services.iter().map(|s| s.to_string()).collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(resolve_scopes(args, project_id, Some(&filter)))
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
    #[serial_test::serial]
    fn resolve_client_credentials_from_env_vars() {
        unsafe {
            std::env::set_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID", "test-id");
            std::env::set_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET", "test-secret");
        }
        let result = resolve_client_credentials();
        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID");
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET");
        }
        let (id, secret, _project_id) = result.unwrap();
        assert_eq!(id, "test-id");
        assert_eq!(secret, "test-secret");
        // project_id may be Some if client_secret.json exists on the machine
    }

    #[test]
    #[serial_test::serial]
    fn resolve_client_credentials_missing_env_vars_uses_config() {
        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID");
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET");
        }
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
    fn config_dir_primary_uses_dot_config() {
        // The primary (non-test) path should be ~/.config/gws.
        // We can't easily test the real function without env override,
        // but we verify the building blocks: home_dir + .config + gws.
        let primary = dirs::home_dir().unwrap().join(".config").join("gws");
        assert!(primary.ends_with(".config/gws") || primary.ends_with(r".config\gws"));
    }

    #[test]
    #[serial_test::serial]
    fn config_dir_fallback_to_legacy() {
        // When GOOGLE_WORKSPACE_CLI_CONFIG_DIR points to a legacy-style dir,
        // config_dir() should return it (simulating the test env override).
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy_gws");
        std::fs::create_dir_all(&legacy).unwrap();

        unsafe {
            std::env::set_var("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", legacy.to_str().unwrap());
        }
        let path = config_dir();
        assert_eq!(path, legacy);
        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CONFIG_DIR");
        }
    }

    #[test]
    #[serial_test::serial]
    fn plain_credentials_path_defaults_to_config_dir() {
        // Without env var, should be in config dir
        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE");
        }
        let path = plain_credentials_path();
        assert!(path.ends_with("credentials.json"));
        assert!(path.starts_with(config_dir()));
    }

    #[test]
    #[serial_test::serial]
    fn plain_credentials_path_respects_env_var() {
        unsafe {
            std::env::set_var(
                "GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE",
                "/tmp/test-creds.json",
            );
        }
        let path = plain_credentials_path();
        assert_eq!(path, PathBuf::from("/tmp/test-creds.json"));
        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE");
        }
    }

    #[test]
    fn token_cache_path_is_in_config_dir() {
        let path = token_cache_path();
        assert!(path.ends_with("token_cache.json"));
        assert!(path.starts_with(config_dir()));
    }

    #[tokio::test]
    async fn handle_auth_command_empty_args_prints_usage() {
        let args: Vec<String> = vec![];
        let result = handle_auth_command(&args).await;
        // Empty args now prints usage and returns Ok
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn handle_auth_command_help_flag_returns_ok() {
        let args = vec!["--help".to_string()];
        let result = handle_auth_command(&args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn handle_auth_command_help_short_flag_returns_ok() {
        let args = vec!["-h".to_string()];
        let result = handle_auth_command(&args).await;
        assert!(result.is_ok());
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
    #[serial_test::serial]
    fn resolve_credentials_fails_without_env_vars_or_config() {
        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID");
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET");
        }
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
    #[serial_test::serial]
    fn resolve_credentials_uses_env_vars_when_present() {
        unsafe {
            std::env::set_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID", "test-id");
            std::env::set_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET", "test-secret");
        }

        let result = resolve_client_credentials();

        // Clean up immediately
        unsafe {
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_ID");
            std::env::remove_var("GOOGLE_WORKSPACE_CLI_CLIENT_SECRET");
        }

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
        // HashMap<String, TokenInfo> format from EncryptedTokenStorage
        let data = r#"{"key":{"access_token":"ya29","refresh_token":"1//tok"}}"#;
        assert_eq!(extract_refresh_token(data), Some("1//tok".to_string()));
    }

    // ── is_workspace_admin_scope tests ──────────────────────────────────

    #[test]
    fn is_workspace_admin_scope_apps_alerts() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/apps.alerts"
        ));
    }

    #[test]
    fn is_workspace_admin_scope_apps_groups_settings() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/apps.groups.settings"
        ));
    }

    #[test]
    fn is_workspace_admin_scope_apps_licensing() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/apps.licensing"
        ));
    }

    #[test]
    fn is_workspace_admin_scope_cloud_identity() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/cloud-identity.groups"
        ));
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/cloud-identity.devices"
        ));
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/cloud-identity.policies"
        ));
    }

    #[test]
    fn is_workspace_admin_scope_ediscovery() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/ediscovery"
        ));
    }

    #[test]
    fn is_workspace_admin_scope_directory_readonly() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/directory.readonly"
        ));
    }

    #[test]
    fn is_workspace_admin_scope_groups() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/groups"
        ));
    }

    #[test]
    fn is_workspace_admin_scope_normal_scopes_not_admin() {
        // Consumer/personal-account scopes must NOT be classified as admin-only
        assert!(!is_workspace_admin_scope(
            "https://www.googleapis.com/auth/drive"
        ));
        assert!(!is_workspace_admin_scope(
            "https://www.googleapis.com/auth/gmail.modify"
        ));
        assert!(!is_workspace_admin_scope(
            "https://www.googleapis.com/auth/calendar"
        ));
        assert!(!is_workspace_admin_scope(
            "https://www.googleapis.com/auth/spreadsheets"
        ));
        assert!(!is_workspace_admin_scope(
            "https://www.googleapis.com/auth/chat.messages"
        ));
    }

    // ── is_workspace_admin_scope – new patterns ─────────────────────────

    #[test]
    fn is_workspace_admin_scope_chat_admin() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/chat.admin.memberships"
        ));
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/chat.admin.memberships.readonly"
        ));
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/chat.admin.spaces"
        ));
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/chat.admin.spaces.readonly"
        ));
    }

    #[test]
    fn is_workspace_admin_scope_classroom() {
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/classroom.courses"
        ));
        assert!(is_workspace_admin_scope(
            "https://www.googleapis.com/auth/classroom.rosters"
        ));
    }

    // ── scope_matches_service tests ──────────────────────────────────────

    #[test]
    fn scope_matches_service_exact_match() {
        let services: HashSet<String> = ["drive"].iter().map(|s| s.to_string()).collect();
        assert!(scope_matches_service(
            "https://www.googleapis.com/auth/drive",
            &services
        ));
    }

    #[test]
    fn scope_matches_service_aliases() {
        let services: HashSet<String> = ["sheets", "docs", "slides"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert!(scope_matches_service(
            "https://www.googleapis.com/auth/spreadsheets",
            &services
        ));
        assert!(scope_matches_service(
            "https://www.googleapis.com/auth/documents",
            &services
        ));
        assert!(scope_matches_service(
            "https://www.googleapis.com/auth/presentations",
            &services
        ));
    }

    #[test]
    fn scope_matches_service_prefix_match() {
        let services: HashSet<String> = ["drive"].iter().map(|s| s.to_string()).collect();
        assert!(scope_matches_service(
            "https://www.googleapis.com/auth/drive.readonly",
            &services
        ));
        assert!(scope_matches_service(
            "https://www.googleapis.com/auth/drive.metadata.readonly",
            &services
        ));
    }

    #[test]
    fn scope_matches_service_no_match() {
        let services: HashSet<String> = ["gmail"].iter().map(|s| s.to_string()).collect();
        assert!(!scope_matches_service(
            "https://www.googleapis.com/auth/drive",
            &services
        ));
    }

    #[test]
    fn scope_matches_service_cloud_platform_always_matches() {
        let services: HashSet<String> = ["drive"].iter().map(|s| s.to_string()).collect();
        assert!(scope_matches_service(
            "https://www.googleapis.com/auth/cloud-platform",
            &services
        ));
    }

    #[test]
    fn scope_matches_service_no_partial_name_collision() {
        // "drive" should NOT match "driveactivity" or similar
        let services: HashSet<String> = ["drive"].iter().map(|s| s.to_string()).collect();
        assert!(!scope_matches_service(
            "https://www.googleapis.com/auth/driveactivity",
            &services
        ));
    }

    // ── services filter integration tests ────────────────────────────────

    #[test]
    fn resolve_scopes_with_services_filter() {
        let args: Vec<String> = vec![];
        let scopes = run_resolve_scopes_with_services(&args, None, &["drive", "gmail"]);
        assert!(!scopes.is_empty());
        for scope in &scopes {
            let short = scope
                .strip_prefix("https://www.googleapis.com/auth/")
                .unwrap_or(scope);
            assert!(
                short.starts_with("drive")
                    || short.starts_with("gmail")
                    || short == "cloud-platform",
                "Unexpected scope with service filter: {scope}"
            );
        }
    }

    #[test]
    fn resolve_scopes_services_filter_unknown_service_ignored() {
        let args: Vec<String> = vec![];
        let scopes = run_resolve_scopes_with_services(&args, None, &["drive", "nonexistent"]);
        assert!(!scopes.is_empty());
        // Should contain drive scope but not be affected by nonexistent
        assert!(scopes.iter().any(|s| s.contains("/auth/drive")));
    }

    #[test]
    fn resolve_scopes_services_takes_priority_with_readonly() {
        let args = vec!["--readonly".to_string()];
        let scopes = run_resolve_scopes_with_services(&args, None, &["drive"]);
        assert!(!scopes.is_empty());
        for scope in &scopes {
            let short = scope
                .strip_prefix("https://www.googleapis.com/auth/")
                .unwrap_or(scope);
            assert!(
                short.starts_with("drive") || short == "cloud-platform",
                "Unexpected scope with service + readonly filter: {scope}"
            );
        }
    }

    #[test]
    fn resolve_scopes_services_takes_priority_with_full() {
        let args = vec!["--full".to_string()];
        let scopes = run_resolve_scopes_with_services(&args, None, &["gmail"]);
        assert!(!scopes.is_empty());
        for scope in &scopes {
            let short = scope
                .strip_prefix("https://www.googleapis.com/auth/")
                .unwrap_or(scope);
            assert!(
                short.starts_with("gmail") || short == "cloud-platform",
                "Unexpected scope with service + full filter: {scope}"
            );
        }
    }

    #[test]
    fn resolve_scopes_explicit_scopes_bypass_services_filter() {
        // --scopes should take priority over -s
        let args = vec![
            "--scopes".to_string(),
            "https://www.googleapis.com/auth/calendar".to_string(),
        ];
        let scopes = run_resolve_scopes_with_services(&args, None, &["drive"]);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0], "https://www.googleapis.com/auth/calendar");
    }

    #[test]
    fn filter_scopes_by_services_none_returns_all() {
        let scopes = vec![
            "https://www.googleapis.com/auth/drive".to_string(),
            "https://www.googleapis.com/auth/gmail.modify".to_string(),
        ];
        let result = filter_scopes_by_services(scopes.clone(), None);
        assert_eq!(result, scopes);
    }

    #[test]
    fn filter_scopes_by_services_empty_set_returns_all() {
        let scopes = vec![
            "https://www.googleapis.com/auth/drive".to_string(),
            "https://www.googleapis.com/auth/gmail.modify".to_string(),
        ];
        let empty: HashSet<String> = HashSet::new();
        let result = filter_scopes_by_services(scopes.clone(), Some(&empty));
        assert_eq!(result, scopes);
    }

    #[test]
    fn filter_restrictive_removes_metadata_when_broader_present() {
        let scopes = vec![
            "https://www.googleapis.com/auth/gmail.modify".to_string(),
            "https://www.googleapis.com/auth/gmail.metadata".to_string(),
            "https://www.googleapis.com/auth/drive".to_string(),
        ];
        let result = filter_redundant_restrictive_scopes(scopes);
        assert!(!result.iter().any(|s| s.contains("gmail.metadata")));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_restrictive_removes_metadata_when_full_gmail_present() {
        let scopes = vec![
            "https://mail.google.com/".to_string(),
            "https://www.googleapis.com/auth/gmail.metadata".to_string(),
        ];
        let result = filter_redundant_restrictive_scopes(scopes);
        assert_eq!(result, vec!["https://mail.google.com/"]);
    }

    #[test]
    fn filter_restrictive_keeps_metadata_when_only_scope() {
        let scopes = vec![
            "https://www.googleapis.com/auth/gmail.metadata".to_string(),
            "https://www.googleapis.com/auth/drive".to_string(),
        ];
        let result = filter_redundant_restrictive_scopes(scopes.clone());
        assert_eq!(result, scopes);
    }

    #[test]
    fn mask_secret_long_string() {
        let masked = mask_secret("GOCSPX-abcdefghijklmnopqrstuvwxyz");
        assert_eq!(masked, "GOCS...wxyz");
    }

    #[test]
    fn mask_secret_short_string() {
        // 8 chars or fewer should be fully masked
        assert_eq!(mask_secret("12345678"), "***");
        assert_eq!(mask_secret("short"), "***");
        assert_eq!(mask_secret(""), "***");
    }

    #[test]
    fn mask_secret_boundary() {
        // Exactly 9 chars — first 4 + last 4 with "..." in between
        assert_eq!(mask_secret("123456789"), "1234...6789");
    }
}

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

//! GCP project setup and OAuth credential bootstrap.
//!
//! Automates the manual GCP setup steps: gcloud auth, project selection,
//! API enabling, consent screen configuration, and OAuth client creation.
//! Uses `gcloud` CLI for project ops and the OAuth2 REST API for credential creation.

use std::process::Command;

use serde_json::json;

use crate::error::GwsError;

use crate::setup_tui::{PickerResult, SelectItem, SetupWizard, StepStatus};

/// A Workspace API with its service ID, human-readable name, and discovery doc coordinates.
struct ApiEntry {
    id: &'static str,
    name: &'static str,
    /// Discovery API name (e.g. "gmail", "drive").
    discovery: &'static str,
    /// Discovery API version (e.g. "v1", "v3").
    version: &'static str,
}

/// All Google Workspace API service IDs that can be enabled.
const WORKSPACE_APIS: &[ApiEntry] = &[
    ApiEntry {
        id: "drive.googleapis.com",
        name: "Google Drive",
        discovery: "drive",
        version: "v3",
    },
    ApiEntry {
        id: "sheets.googleapis.com",
        name: "Google Sheets",
        discovery: "sheets",
        version: "v4",
    },
    ApiEntry {
        id: "gmail.googleapis.com",
        name: "Gmail",
        discovery: "gmail",
        version: "v1",
    },
    ApiEntry {
        id: "calendar-json.googleapis.com",
        name: "Google Calendar",
        discovery: "calendar",
        version: "v3",
    },
    ApiEntry {
        id: "docs.googleapis.com",
        name: "Google Docs",
        discovery: "docs",
        version: "v1",
    },
    ApiEntry {
        id: "slides.googleapis.com",
        name: "Google Slides",
        discovery: "slides",
        version: "v1",
    },
    ApiEntry {
        id: "tasks.googleapis.com",
        name: "Google Tasks",
        discovery: "tasks",
        version: "v1",
    },
    ApiEntry {
        id: "people.googleapis.com",
        name: "People (Contacts)",
        discovery: "people",
        version: "v1",
    },
    ApiEntry {
        id: "chat.googleapis.com",
        name: "Google Chat",
        discovery: "chat",
        version: "v1",
    },
    ApiEntry {
        id: "vault.googleapis.com",
        name: "Google Vault",
        discovery: "vault",
        version: "v1",
    },
    ApiEntry {
        id: "groupssettings.googleapis.com",
        name: "Groups Settings",
        discovery: "groupssettings",
        version: "v1",
    },
    ApiEntry {
        id: "reseller.googleapis.com",
        name: "Reseller",
        discovery: "reseller",
        version: "v1",
    },
    ApiEntry {
        id: "licensing.googleapis.com",
        name: "Licensing",
        discovery: "licensing",
        version: "v1",
    },
    ApiEntry {
        id: "script.googleapis.com",
        name: "Apps Script",
        discovery: "script",
        version: "v1",
    },
    ApiEntry {
        id: "admin.googleapis.com",
        name: "Admin SDK",
        discovery: "admin",
        version: "directory_v1",
    },
    ApiEntry {
        id: "classroom.googleapis.com",
        name: "Classroom",
        discovery: "classroom",
        version: "v1",
    },
    ApiEntry {
        id: "cloudidentity.googleapis.com",
        name: "Cloud Identity",
        discovery: "cloudidentity",
        version: "v1",
    },
    ApiEntry {
        id: "alertcenter.googleapis.com",
        name: "Alert Center",
        discovery: "alertcenter",
        version: "v1beta1",
    },
    ApiEntry {
        id: "forms.googleapis.com",
        name: "Google Forms",
        discovery: "forms",
        version: "v1",
    },
    ApiEntry {
        id: "keep.googleapis.com",
        name: "Google Keep",
        discovery: "keep",
        version: "v1",
    },
    ApiEntry {
        id: "meet.googleapis.com",
        name: "Google Meet",
        discovery: "meet",
        version: "v2",
    },
    ApiEntry {
        id: "pubsub.googleapis.com",
        name: "Cloud Pub/Sub",
        discovery: "pubsub",
        version: "v1",
    },
];

const RESTRICTED_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/chat.admin.delete",
    "https://www.googleapis.com/auth/chat.delete",
    "https://www.googleapis.com/auth/chat.messages",
    "https://www.googleapis.com/auth/chat.messages.readonly",
    "https://www.googleapis.com/auth/drive",
    "https://www.googleapis.com/auth/drive.activity",
    "https://www.googleapis.com/auth/drive.activity.readonly",
    "https://www.googleapis.com/auth/drive.meet.readonly",
    "https://www.googleapis.com/auth/drive.metadata",
    "https://www.googleapis.com/auth/drive.metadata.readonly",
    "https://www.googleapis.com/auth/drive.readonly",
    "https://www.googleapis.com/auth/drive.scripts",
    "https://www.googleapis.com/auth/gmail.compose",
    "https://www.googleapis.com/auth/gmail.insert",
    "https://www.googleapis.com/auth/gmail.metadata",
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/gmail.settings.basic",
    "https://www.googleapis.com/auth/gmail.settings.sharing",
];

const SENSITIVE_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/chat.admin.memberships",
    "https://www.googleapis.com/auth/chat.admin.memberships.readonly",
    "https://www.googleapis.com/auth/chat.admin.spaces",
    "https://www.googleapis.com/auth/chat.admin.spaces.readonly",
    "https://www.googleapis.com/auth/chat.customemojis",
    "https://www.googleapis.com/auth/chat.customemojis.readonly",
    "https://www.googleapis.com/auth/documents",
    "https://www.googleapis.com/auth/documents.readonly",
    "https://www.googleapis.com/auth/chat.memberships",
    "https://www.googleapis.com/auth/chat.memberships.app",
    "https://www.googleapis.com/auth/chat.memberships.readonly",
    "https://www.googleapis.com/auth/chat.messages.create",
    "https://www.googleapis.com/auth/chat.messages.reactions",
    "https://www.googleapis.com/auth/chat.messages.reactions.create",
    "https://www.googleapis.com/auth/chat.messages.reactions.readonly",
    "https://www.googleapis.com/auth/chat.spaces",
    "https://www.googleapis.com/auth/chat.spaces.create",
    "https://www.googleapis.com/auth/chat.spaces.readonly",
    "https://www.googleapis.com/auth/chat.users.readstate",
    "https://www.googleapis.com/auth/chat.users.readstate.readonly",
    "https://www.googleapis.com/auth/chat.users.spacesettings",
    "https://www.googleapis.com/auth/drive.apps.readonly",
    "https://www.googleapis.com/auth/gmail.addons.current.message.metadata",
    "https://www.googleapis.com/auth/gmail.addons.current.message.readonly",
    "https://www.googleapis.com/auth/gmail.send",
];

/// Helper to get just the API IDs (for tests and non-interactive mode).
fn all_api_ids() -> Vec<&'static str> {
    WORKSPACE_APIS.iter().map(|a| a.id).collect()
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeClassification {
    NonSensitive,
    Sensitive,
    Restricted,
}

pub const PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// A scope discovered from a Discovery Document.
#[derive(Clone)]
pub struct DiscoveredScope {
    /// Full scope URL, e.g. "https://www.googleapis.com/auth/drive"
    pub url: String,
    /// Short label, e.g. "drive"
    pub short: String,
    /// Human-readable description from the Discovery Document.
    pub description: String,
    /// Which API this scope came from, e.g. "Google Drive"
    #[allow(dead_code)]
    pub api_name: String,
    /// Whether this is a ".readonly" variant.
    pub is_readonly: bool,
    /// Sensitivity classification.
    pub classification: ScopeClassification,
}

/// Fetch scopes from discovery docs for the given enabled API IDs.
pub async fn fetch_scopes_for_apis(enabled_api_ids: &[String]) -> Vec<DiscoveredScope> {
    let mut all_scopes: Vec<DiscoveredScope> = Vec::new();

    for api_entry in WORKSPACE_APIS {
        if !enabled_api_ids.iter().any(|id| id == api_entry.id) {
            continue;
        }
        let doc = match crate::discovery::fetch_discovery_document(
            api_entry.discovery,
            api_entry.version,
        )
        .await
        {
            Ok(d) => d,
            Err(_) => continue, // skip APIs we can't find a discovery doc for
        };

        if let Some(auth) = &doc.auth {
            if let Some(oauth2) = &auth.oauth2 {
                if let Some(scopes) = &oauth2.scopes {
                    for (url, desc) in scopes {
                        // Deduplicate (some APIs share scopes)
                        if all_scopes.iter().any(|s| s.url == *url) {
                            continue;
                        }

                        // Filter out legacy endpoints like m8/feeds or calendar/feeds
                        if !url.starts_with("https://www.googleapis.com/auth/") {
                            continue;
                        }
                        // Filter out app-only scopes that can't be used with user OAuth consent
                        // (they require a Chat app / service account)
                        if url.contains("/auth/chat.app.")
                            || url.contains("/auth/chat.bot")
                            || url.contains("/auth/chat.import")
                            || url.contains("/auth/keep")
                        {
                            continue;
                        }
                        let short = url
                            .strip_prefix("https://www.googleapis.com/auth/")
                            .unwrap_or(url)
                            .to_string();
                        let is_readonly = short.contains("readonly");

                        let classification = if RESTRICTED_SCOPES.contains(&url.as_str()) {
                            ScopeClassification::Restricted
                        } else if SENSITIVE_SCOPES.contains(&url.as_str()) {
                            ScopeClassification::Sensitive
                        } else {
                            ScopeClassification::NonSensitive
                        };

                        let description = if let Some(desc) = &desc.description {
                            if !desc.is_empty() {
                                desc.clone()
                            } else {
                                // Generate a friendly name from the short URL
                                short
                                    .split('.')
                                    .map(|s| {
                                        let mut c = s.chars();
                                        match c.next() {
                                            None => String::new(),
                                            Some(f) => {
                                                f.to_uppercase().collect::<String>() + c.as_str()
                                            }
                                        }
                                    })
                                    .collect::<Vec<String>>()
                                    .join(" ")
                            }
                        } else {
                            // Generate a friendly name from the short URL
                            short
                                .split('.')
                                .map(|s| {
                                    let mut c = s.chars();
                                    match c.next() {
                                        None => String::new(),
                                        Some(f) => {
                                            f.to_uppercase().collect::<String>() + c.as_str()
                                        }
                                    }
                                })
                                .collect::<Vec<String>>()
                                .join(" ")
                        };

                        all_scopes.push(DiscoveredScope {
                            url: url.clone(),
                            description,
                            short,
                            is_readonly,
                            api_name: api_entry.name.to_string(),
                            classification,
                        });
                    }
                }
            }
        }
    }

    // Sort: restricted first, then sensitive, then non-sensitive, then alphabetically
    all_scopes.sort_by(|a, b| {
        b.classification
            .cmp(&a.classification)
            .then_with(|| a.short.cmp(&b.short))
    });

    all_scopes
}

/// Options for the setup command.
pub struct SetupOptions {
    pub project: Option<String>,
    pub dry_run: bool,
}

/// Parse setup flags from args.
pub fn parse_setup_args(args: &[String]) -> SetupOptions {
    let mut project = None;
    let mut dry_run = false;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--project" && i + 1 < args.len() {
            project = Some(args[i + 1].clone());
            i += 2;
        } else if args[i].starts_with("--project=") {
            project = Some(args[i].split_once('=').unwrap().1.to_string());
            i += 1;
        } else if args[i] == "--dry-run" {
            dry_run = true;
            i += 1;
        } else {
            i += 1;
        }
    }
    SetupOptions { project, dry_run }
}

// ── gcloud helpers ──────────────────────────────────────────────

/// Create a gcloud Command with interactive prompts disabled.
/// This prevents CBA proxy install prompts from blocking subprocess calls.
fn gcloud_cmd() -> Command {
    let mut cmd = Command::new("gcloud");
    cmd.env("CLOUDSDK_CORE_DISABLE_PROMPTS", "1");
    cmd
}

/// Check if gcloud CLI is installed.
pub fn is_gcloud_installed() -> bool {
    Command::new("gcloud")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `gcloud auth login` interactively.
fn gcloud_auth_login() -> Result<(), GwsError> {
    let status = gcloud_cmd()
        .args(["auth", "login"])
        .status()
        .map_err(|e| GwsError::Auth(format!("Failed to run gcloud auth login: {e}")))?;

    if !status.success() {
        return Err(GwsError::Auth("gcloud auth login failed".to_string()));
    }
    Ok(())
}

/// Get the active gcloud account email.
fn get_gcloud_account() -> Result<Option<String>, GwsError> {
    let output = gcloud_cmd()
        .args(["config", "get-value", "account"])
        .output()
        .map_err(|e| GwsError::Auth(format!("Failed to run gcloud: {e}")))?;

    if !output.status.success() {
        return Ok(None);
    }

    let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if val.is_empty() || val == "(unset)" {
        return Ok(None);
    }
    Ok(Some(val))
}

/// List all authenticated gcloud accounts.
/// Returns (account_email, is_active) pairs.
fn list_gcloud_accounts() -> Vec<(String, bool)> {
    let output = gcloud_cmd()
        .args(["auth", "list", "--format=value(account,status)"])
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(2, '\t').collect();
                if parts.is_empty() || parts[0].is_empty() {
                    None
                } else {
                    let account = parts[0].to_string();
                    let active = parts.get(1).is_some_and(|s| s.contains("ACTIVE"));
                    Some((account, active))
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Set the active gcloud account.
fn set_gcloud_account(account: &str) -> Result<(), GwsError> {
    let status = gcloud_cmd()
        .args(["config", "set", "account", account])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| GwsError::Auth(format!("Failed to set account: {e}")))?;
    if !status.success() {
        return Err(GwsError::Auth(format!(
            "Failed to set account to '{account}'"
        )));
    }
    Ok(())
}

/// Get the current gcloud project ID.
fn get_gcloud_project() -> Result<Option<String>, GwsError> {
    let output = gcloud_cmd()
        .args(["config", "get-value", "project"])
        .output()
        .map_err(|e| GwsError::Auth(format!("Failed to run gcloud: {e}")))?;

    if !output.status.success() {
        return Ok(None);
    }

    let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if val.is_empty() || val == "(unset)" {
        return Ok(None);
    }
    Ok(Some(val))
}

/// Set the active gcloud project.
fn set_gcloud_project(project_id: &str) -> Result<(), GwsError> {
    let status = gcloud_cmd()
        .args(["config", "set", "project", project_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| GwsError::Validation(format!("Failed to set gcloud project: {e}")))?;

    if !status.success() {
        return Err(GwsError::Validation(format!(
            "Failed to set project to '{project_id}'"
        )));
    }
    Ok(())
}

/// List all GCP projects accessible to the current user.
/// Returns a list of (project_id, project_name) tuples, and an optional error message.
/// Times out after 10 seconds to avoid hanging on CBA-enrolled devices.
/// gcloud stderr flows through to the terminal so users see progress/error messages.
fn list_gcloud_projects() -> (Vec<(String, String)>, Option<String>) {
    let child = gcloud_cmd()
        .args([
            "projects",
            "list",
            "--format=value(projectId,name)",
            "--sort-by=projectId",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit()) // let user see gcloud messages
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (Vec::new(), Some(format!("Failed to run gcloud: {e}"))),
    };

    // Wait with timeout
    let timeout = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    let stdout = child
                        .stdout
                        .take()
                        .map(|mut s| {
                            let mut buf = String::new();
                            std::io::Read::read_to_string(&mut s, &mut buf).ok();
                            buf
                        })
                        .unwrap_or_default();
                    let projects = stdout
                        .lines()
                        .filter_map(|line| {
                            let parts: Vec<&str> = line.splitn(2, '\t').collect();
                            if parts.is_empty() || parts[0].is_empty() {
                                None
                            } else {
                                let id = parts[0].to_string();
                                let name = parts.get(1).unwrap_or(&"").to_string();
                                Some((id, name))
                            }
                        })
                        .collect();
                    return (projects, None);
                } else {
                    return (
                        Vec::new(),
                        Some("gcloud projects list failed (see above)".to_string()),
                    );
                }
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return (
                        Vec::new(),
                        Some("Timed out listing projects (10s)".to_string()),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return (Vec::new(), Some(format!("Error waiting for gcloud: {e}"))),
        }
    }
}

/// Get a gcloud access token for REST API calls.
fn get_access_token() -> Result<String, GwsError> {
    let output = gcloud_cmd()
        .args(["auth", "print-access-token"])
        .output()
        .map_err(|e| GwsError::Auth(format!("Failed to get access token: {e}")))?;

    if !output.status.success() {
        return Err(GwsError::Auth(
            "Failed to get gcloud access token. Run `gcloud auth login` first.".to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ── API enabling ────────────────────────────────────────────────

/// Enable selected Workspace APIs for a project. Returns (enabled, skipped, failed).
async fn enable_apis(
    project_id: &str,
    api_ids: &[String],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    // First, get already-enabled APIs
    let already_enabled = get_enabled_apis(project_id);

    let mut to_enable = Vec::new();
    let mut skipped = Vec::new();

    for api_id in api_ids {
        if already_enabled.contains(api_id) {
            skipped.push(api_id.clone());
        } else {
            to_enable.push(api_id.clone());
        }
    }

    if to_enable.is_empty() {
        return (Vec::new(), skipped, Vec::new());
    }

    // Batch enable all APIs in a single gcloud call
    let mut args = vec!["services".to_string(), "enable".to_string()];
    args.extend(to_enable.clone());
    args.push("--project".to_string());
    args.push(project_id.to_string());

    let result = gcloud_cmd()
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    match result {
        Ok(output) if output.status.success() => (to_enable, skipped, Vec::new()),
        Ok(_) => (Vec::new(), skipped, to_enable),
        Err(_) => (Vec::new(), skipped, to_enable),
    }
}

/// Get the list of already-enabled API service names for a project.
pub fn get_enabled_apis(project_id: &str) -> Vec<String> {
    let output = gcloud_cmd()
        .args([
            "services",
            "list",
            "--enabled",
            "--project",
            project_id,
            "--format=json",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let json_str = String::from_utf8_lossy(&out.stdout);
            if let Ok(services) = serde_json::from_str::<Vec<serde_json::Value>>(&json_str) {
                return services
                    .iter()
                    .filter_map(|s| {
                        s.get("config")
                            .and_then(|c| c.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

// ── OAuth REST API ──────────────────────────────────────────────

/// Configure the OAuth consent screen via REST API.
async fn configure_consent_screen(
    project_id: &str,
    access_token: &str,
    app_name: &str,
    support_email: &str,
) -> Result<(), GwsError> {
    let client = crate::client::build_client()?;

    // Check if consent screen already exists
    let check_url = format!(
        "https://oauth2.googleapis.com/v1/projects/{}/brands",
        project_id
    );

    let check_res = client
        .get(&check_url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| GwsError::Auth(format!("Failed to check consent screen: {e}")))?;

    if check_res.status().is_success() {
        let data: serde_json::Value = check_res.json().await.unwrap_or_else(|_| json!({}));
        if let Some(brands) = data.get("brands").and_then(|b| b.as_array()) {
            if !brands.is_empty() {
                return Ok(());
            }
        }
    }

    // Create the consent screen
    let create_res = client
        .post(&check_url)
        .bearer_auth(access_token)
        .json(&json!({
            "applicationTitle": app_name,
            "supportEmail": support_email,
        }))
        .send()
        .await
        .map_err(|e| GwsError::Auth(format!("Failed to create consent screen: {e}")))?;

    if create_res.status().is_success() {
        return Ok(());
    }

    let body = create_res.text().await.unwrap_or_default();
    if body.contains("already exists") || body.contains("ALREADY_EXISTS") {
        return Ok(());
    }

    // Fallback to manual instructions.
    // We don't print anything here because the TUI / CLI orchestrator
    // will guide the user to check/configure the consent screen.
    Ok(())
}

// (create_oauth_client removed due to IAP Admin APIs deprecation)

// ── Main setup orchestrator ─────────────────────────────────────

const STEP_LABELS: [&str; 5] = [
    "gcloud CLI",
    "Authentication",
    "GCP project",
    "Workspace APIs",
    "OAuth credentials",
];

enum SetupStage {
    CheckGcloud,
    Account,
    Project,
    EnableApis,
    ConfigureOauth,
    Finish,
}

/// Shared mutable state threaded through each setup stage.
struct SetupContext {
    wizard: Option<SetupWizard>,
    interactive: bool,
    dry_run: bool,
    opts: SetupOptions,
    account: String,
    project_id: String,
    api_ids: Vec<String>,
    client_id: String,
    client_secret: String,
    enabled: Vec<String>,
    skipped: Vec<String>,
    failed: Vec<String>,
}

impl SetupContext {
    /// Helper to update wizard step if present.
    fn wiz(&mut self, idx: usize, status: StepStatus) {
        if let Some(ref mut w) = self.wizard {
            let _ = w.update_step(idx, status);
        }
    }

    /// Finish and consume the wizard.
    fn finish_wizard(&mut self) {
        if let Some(w) = self.wizard.take() {
            let _ = w.finish();
        }
    }
}

/// Stage 1: Verify that gcloud CLI is installed.
fn stage_check_gcloud(ctx: &mut SetupContext) -> Result<SetupStage, GwsError> {
    ctx.wiz(0, StepStatus::InProgress("Checking...".into()));
    if !ctx.dry_run {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if !is_gcloud_installed() {
        ctx.wiz(0, StepStatus::Failed("not found".into()));
        ctx.finish_wizard();
        return Err(GwsError::Validation(
            "gcloud CLI not found. Install it from https://cloud.google.com/sdk/docs/install"
                .to_string(),
        ));
    }
    ctx.wiz(0, StepStatus::Done("found".into()));
    if !ctx.interactive {
        eprintln!("Step 1/6: Checking for gcloud CLI...\n  ✓ gcloud CLI found");
    }
    Ok(SetupStage::Account)
}

/// Stage 2: Select or authenticate a Google account.
fn stage_account(ctx: &mut SetupContext) -> Result<SetupStage, GwsError> {
    ctx.wiz(1, StepStatus::InProgress(String::new()));
    if ctx.interactive {
        let accounts = list_gcloud_accounts();
        let current = get_gcloud_account()?.unwrap_or_default();

        let mut items: Vec<SelectItem> = vec![SelectItem {
            label: "➕ Login with new account".to_string(),
            description: "Opens browser for gcloud auth login".to_string(),
            selected: false,
            is_fixed: false,
            is_template: false,
            template_selects: vec![],
        }];
        items.extend(accounts.iter().map(|(acct, active)| SelectItem {
            label: acct.clone(),
            description: if *active {
                "(active)".to_string()
            } else {
                String::new()
            },
            selected: *acct == current,
            is_fixed: false,
            is_template: false,
            template_selects: vec![],
        }));

        let result = ctx
            .wizard
            .as_mut()
            .unwrap()
            .show_picker(
                "Select a Google account",
                "Space to select, Enter to confirm",
                items,
                false,
            )
            .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?;

        match result {
            PickerResult::Confirmed(items) => {
                let chosen = items.iter().find(|i| i.selected);
                match chosen {
                    Some(item) if item.label.starts_with('➕') => {
                        ctx.wizard
                            .as_mut()
                            .unwrap()
                            .suspend()
                            .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?;
                        eprintln!("  → Opening browser for login...");
                        gcloud_auth_login()?;
                        let acct = get_gcloud_account()?.ok_or_else(|| {
                            GwsError::Auth("Authentication failed — no active account".to_string())
                        })?;
                        ctx.wizard
                            .as_mut()
                            .unwrap()
                            .resume()
                            .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?;
                        ctx.wiz(1, StepStatus::Done(acct.clone()));
                        ctx.account = acct;
                        Ok(SetupStage::Project)
                    }
                    Some(item) => {
                        set_gcloud_account(&item.label)?;
                        ctx.wiz(1, StepStatus::Done(item.label.clone()));
                        ctx.account = item.label.clone();
                        Ok(SetupStage::Project)
                    }
                    None => {
                        ctx.finish_wizard();
                        Err(GwsError::Validation("No account selected".to_string()))
                    }
                }
            }
            PickerResult::GoBack => Ok(SetupStage::CheckGcloud),
            PickerResult::Cancelled => {
                ctx.finish_wizard();
                Err(GwsError::Validation("Setup cancelled".to_string()))
            }
        }
    } else {
        ctx.account = match get_gcloud_account()? {
            Some(acct) => {
                eprintln!(
                    "Step 2/6: Checking authentication...\n  ✓ Authenticated as {}",
                    acct
                );
                acct
            }
            None => {
                eprintln!(
                    "Step 2/6: Checking authentication...\n  → Not logged in. Running gcloud auth login..."
                );
                gcloud_auth_login()?;
                get_gcloud_account()?.ok_or_else(|| {
                    GwsError::Auth("Authentication failed — no active account".to_string())
                })?
            }
        };
        Ok(SetupStage::Project)
    }
}

/// Stage 3: Select or create a GCP project.
fn stage_project(ctx: &mut SetupContext) -> Result<SetupStage, GwsError> {
    ctx.wiz(2, StepStatus::InProgress(String::new()));
    if let Some(p) = ctx.opts.project.clone() {
        if !ctx.dry_run {
            set_gcloud_project(&p)?;
        }
        ctx.wiz(2, StepStatus::Done(p.clone()));
        if !ctx.interactive {
            eprintln!("Step 3/6: Project set to {}", p);
        }
        ctx.project_id = p;
        return Ok(SetupStage::EnableApis);
    }

    if ctx.interactive {
        if let Some(ref mut w) = ctx.wizard {
            let _ = w.show_message("Loading projects...");
        }
        let (projects, list_err) = list_gcloud_projects();
        if let Some(err) = &list_err {
            if let Some(ref mut w) = ctx.wizard {
                let _ = w.show_message(&format!("⚠ Could not list projects: {err}"));
            }
        }
        let current = get_gcloud_project()?.unwrap_or_default();

        let mut items: Vec<SelectItem> = vec![SelectItem {
            label: "➕ Create new project".to_string(),
            description: "Create a new GCP project for gws".to_string(),
            selected: false,
            is_fixed: false,
            is_template: false,
            template_selects: vec![],
        }];
        items.extend(projects.iter().map(|(id, name)| SelectItem {
            label: id.clone(),
            description: name.clone(),
            selected: *id == current,
            is_fixed: false,
            is_template: false,
            template_selects: vec![],
        }));

        let result = ctx
            .wizard
            .as_mut()
            .unwrap()
            .show_picker(
                "Select a GCP project",
                "Space to select, Enter to confirm",
                items,
                false,
            )
            .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?;

        match result {
            PickerResult::Confirmed(items) => {
                let chosen = items.iter().find(|i| i.selected);
                match chosen {
                    Some(item) if item.label.starts_with('➕') => {
                        let project_name = match ctx
                            .wizard
                            .as_mut()
                            .unwrap()
                            .show_input("Create new GCP project", "Enter a unique project ID", None)
                            .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?
                        {
                            crate::setup_tui::InputResult::Confirmed(v) if !v.is_empty() => v,
                            _ => {
                                return Err(GwsError::Validation(
                                    "Project creation cancelled by user".to_string(),
                                ))
                            }
                        };

                        ctx.wizard
                            .as_mut()
                            .unwrap()
                            .show_message(&format!("Creating project '{}'...", project_name))
                            .ok();

                        let status = gcloud_cmd()
                            .args(["projects", "create", &project_name])
                            .status()
                            .map_err(|e| {
                                GwsError::Validation(format!("Failed to create project: {e}"))
                            })?;
                        if !status.success() {
                            return Err(GwsError::Validation(format!(
                                "Failed to create project '{}'. Check the ID is valid and unique.",
                                project_name
                            )));
                        }
                        set_gcloud_project(&project_name)?;
                        ctx.wiz(2, StepStatus::Done(project_name.clone()));
                        ctx.project_id = project_name;
                        Ok(SetupStage::EnableApis)
                    }
                    Some(item) => {
                        set_gcloud_project(&item.label)?;
                        ctx.wiz(2, StepStatus::Done(item.label.clone()));
                        ctx.project_id = item.label.clone();
                        Ok(SetupStage::EnableApis)
                    }
                    None => {
                        ctx.finish_wizard();
                        Err(GwsError::Validation(
                            "No project selected. Use --project <id> to specify one.".to_string(),
                        ))
                    }
                }
            }
            PickerResult::GoBack => Ok(SetupStage::Account),
            PickerResult::Cancelled => {
                ctx.finish_wizard();
                Err(GwsError::Validation("Setup cancelled".to_string()))
            }
        }
    } else {
        ctx.project_id = match get_gcloud_project()? {
            Some(p) => {
                eprintln!("Step 3/6: Using current project: {}", p);
                p
            }
            None => {
                return Err(GwsError::Validation(
                    "No GCP project configured. Use --project <id> or run `gcloud config set project <id>`"
                        .to_string(),
                ));
            }
        };
        Ok(SetupStage::EnableApis)
    }
}

/// Stage 4: Select and enable Workspace APIs.
async fn stage_enable_apis(ctx: &mut SetupContext) -> Result<SetupStage, GwsError> {
    ctx.wiz(3, StepStatus::InProgress(String::new()));
    if ctx.interactive {
        let already_enabled = get_enabled_apis(&ctx.project_id);
        let items: Vec<SelectItem> = WORKSPACE_APIS
            .iter()
            .map(|api| {
                let already = already_enabled.contains(&api.id.to_string());
                SelectItem {
                    label: api.name.to_string(),
                    description: if already {
                        format!("{} (already enabled)", api.id)
                    } else {
                        api.id.to_string()
                    },
                    selected: already,
                    is_fixed: already,
                    is_template: false,
                    template_selects: vec![],
                }
            })
            .collect();

        let result = ctx
            .wizard
            .as_mut()
            .unwrap()
            .show_picker(
                "Select APIs to enable",
                "Space to toggle, 'a' to select all, Enter to confirm",
                items,
                true,
            )
            .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?;

        match result {
            PickerResult::Confirmed(items) => {
                ctx.api_ids = items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.selected)
                    .map(|(i, _)| WORKSPACE_APIS[i].id.to_string())
                    .collect::<Vec<_>>();
            }
            PickerResult::GoBack => {
                return Ok(SetupStage::Project);
            }
            PickerResult::Cancelled => {
                ctx.finish_wizard();
                return Err(GwsError::Validation("Setup cancelled".to_string()));
            }
        }
    } else {
        ctx.api_ids = all_api_ids().iter().map(|s| s.to_string()).collect();
    }

    if ctx.dry_run {
        eprintln!("Step 4/5: Would enable {} APIs:", ctx.api_ids.len());
        for id in &ctx.api_ids {
            eprintln!("  - {}", id);
        }
        eprintln!("Step 5/5: Would configure OAuth credentials (Consent + Client)");
        eprintln!();
        let output = json!({
            "status": "dry_run",
            "message": "No changes were made. Run `gws auth login` to authenticate.",
            "account": ctx.account,
            "project": ctx.project_id,
            "apis_would_enable": ctx.api_ids,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
        return Ok(SetupStage::Finish);
    }

    ctx.wiz(
        3,
        StepStatus::InProgress(format!("Enabling {} APIs...", ctx.api_ids.len())),
    );
    let (enabled_apis, skipped_apis, failed_apis) =
        enable_apis(&ctx.project_id, &ctx.api_ids).await;
    ctx.enabled = enabled_apis;
    ctx.skipped = skipped_apis;
    ctx.failed = failed_apis;
    ctx.wiz(
        3,
        StepStatus::Done(format!(
            "{} enabled, {} skipped",
            ctx.enabled.len(),
            ctx.skipped.len()
        )),
    );
    Ok(SetupStage::ConfigureOauth)
}

/// Stage 5: Configure OAuth consent screen and collect client credentials.
async fn stage_configure_oauth(ctx: &mut SetupContext) -> Result<SetupStage, GwsError> {
    ctx.wiz(4, StepStatus::InProgress("Configuring...".into()));
    let access_token = get_access_token()?;
    let app_name = "gws CLI";
    configure_consent_screen(&ctx.project_id, &access_token, app_name, &ctx.account).await?;

    ctx.wiz(
        4,
        StepStatus::InProgress("Waiting for manual input...".into()),
    );
    if !ctx.interactive {
        return Err(GwsError::Validation(
            "Cannot automate OAuth client creation. Please run setup interactively.".to_string(),
        ));
    }

    let (cid_result, csecret_result) = if let Some(ref mut w) = ctx.wizard {
        let current_creds: Option<serde_json::Value> = crate::credential_store::load_encrypted()
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());

        w.show_message(&format!(
            "Go to: https://console.cloud.google.com/apis/credentials/consent?project={}\n\
             Ensure 'External' consent screen is configured. Then,\n\
             Go to: https://console.cloud.google.com/apis/credentials?project={}\n\
             Click 'Create Credentials' -> 'OAuth client ID' -> 'Desktop app'",
            ctx.project_id, ctx.project_id
        ))
        .ok();

        let cid_res = w
            .show_input(
                "Enter OAuth Client ID",
                "Paste the Client ID from Google Cloud Console",
                current_creds.as_ref().and_then(|c| c["client_id"].as_str()),
            )
            .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?;

        let csec_res = match &cid_res {
            crate::setup_tui::InputResult::Confirmed(v) if !v.is_empty() => w
                .show_input(
                    "Enter OAuth Client Secret",
                    "Paste the Client Secret from Google Cloud Console",
                    current_creds
                        .as_ref()
                        .and_then(|c| c["client_secret"].as_str()),
                )
                .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?,
            _ => crate::setup_tui::InputResult::Cancelled,
        };
        (cid_res, csec_res)
    } else {
        return Err(GwsError::Validation(
            "Interactive mode required for OAuth client input".to_string(),
        ));
    };

    ctx.client_id = match cid_result {
        crate::setup_tui::InputResult::Confirmed(v) => {
            if v.is_empty() {
                ctx.finish_wizard();
                return Err(GwsError::Validation("Client ID cannot be empty".into()));
            }
            v
        }
        crate::setup_tui::InputResult::GoBack => {
            return Ok(SetupStage::EnableApis);
        }
        crate::setup_tui::InputResult::Cancelled => {
            ctx.finish_wizard();
            return Err(GwsError::Validation("Setup cancelled".into()));
        }
    };

    ctx.client_secret = match csecret_result {
        crate::setup_tui::InputResult::Confirmed(v) => {
            if v.is_empty() {
                ctx.finish_wizard();
                return Err(GwsError::Validation("Client Secret cannot be empty".into()));
            }
            v
        }
        crate::setup_tui::InputResult::GoBack => {
            return Ok(SetupStage::EnableApis);
        }
        crate::setup_tui::InputResult::Cancelled => {
            ctx.finish_wizard();
            return Err(GwsError::Validation("Setup cancelled".into()));
        }
    };

    let _config_path = crate::oauth_config::save_client_config(
        &ctx.client_id,
        &ctx.client_secret,
        &ctx.project_id,
    )
    .map_err(|e| GwsError::Validation(format!("Failed to save client config: {e}")))?;

    ctx.wiz(4, StepStatus::Done("configured".into()));
    Ok(SetupStage::Finish)
}

/// Run the full setup flow. Orchestrates all steps and outputs JSON summary.
pub async fn run_setup(args: &[String]) -> Result<(), GwsError> {
    let opts = parse_setup_args(args);
    let dry_run = opts.dry_run;
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin()) && !dry_run;

    if dry_run {
        eprintln!("🏃 DRY RUN — no changes will be made\n");
    }

    let wizard = if interactive {
        Some(
            SetupWizard::start(&STEP_LABELS)
                .map_err(|e| GwsError::Validation(format!("TUI error: {e}")))?,
        )
    } else {
        None
    };

    let mut ctx = SetupContext {
        wizard,
        interactive,
        dry_run,
        opts,
        account: String::new(),
        project_id: String::new(),
        api_ids: Vec::new(),
        client_id: String::new(),
        client_secret: String::new(),
        enabled: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
    };

    let mut stage = SetupStage::CheckGcloud;

    loop {
        stage = match stage {
            SetupStage::CheckGcloud => stage_check_gcloud(&mut ctx)?,
            SetupStage::Account => stage_account(&mut ctx)?,
            SetupStage::Project => stage_project(&mut ctx)?,
            SetupStage::EnableApis => stage_enable_apis(&mut ctx).await?,
            SetupStage::ConfigureOauth => stage_configure_oauth(&mut ctx).await?,
            SetupStage::Finish => break,
        };
    }

    ctx.finish_wizard();

    let output = json!({
        "status": "success",
        "message": "Setup complete! Run `gws auth login` to authenticate.",
        "account": ctx.account,
        "project": ctx.project_id,
        "apis_enabled": ctx.enabled.len(),
        "apis_skipped": ctx.skipped.len(),
        "apis_failed": ctx.failed.len(),
        "client_config": crate::oauth_config::client_config_path().display().to_string(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_default()
    );

    eprintln!("\n✅ Setup complete! Run `gws auth login` to authenticate.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services;
    use crate::setup_tui::{PickerResult, SelectItem};
    use crossterm::event::KeyCode;

    // ── Action resolution (test-only) ───────────────────────────

    #[derive(Debug, PartialEq)]
    enum SetupAction {
        SetAccount(String),
        LoginNewAccount,
        SetProject(String),
        CreateProject(String),
        EnableApis(Vec<String>),
        NoSelection,
    }

    fn resolve_account_selection(items: &[SelectItem]) -> SetupAction {
        match items.iter().find(|i| i.selected) {
            Some(item) if item.label.starts_with('➕') => SetupAction::LoginNewAccount,
            Some(item) => SetupAction::SetAccount(item.label.clone()),
            None => SetupAction::NoSelection,
        }
    }

    fn resolve_project_selection(items: &[SelectItem]) -> SetupAction {
        match items.iter().find(|i| i.selected) {
            Some(item) if item.label.starts_with('➕') => {
                SetupAction::CreateProject(String::new())
            }
            Some(item) => SetupAction::SetProject(item.label.clone()),
            None => SetupAction::NoSelection,
        }
    }

    fn resolve_api_selection(items: &[SelectItem]) -> SetupAction {
        let api_ids: Vec<String> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.selected)
            .filter_map(|(i, _)| WORKSPACE_APIS.get(i).map(|a| a.id.to_string()))
            .collect();
        SetupAction::EnableApis(api_ids)
    }

    // ── Helpers ─────────────────────────────────────────────────

    fn make_items(labels: &[&str]) -> Vec<SelectItem> {
        labels
            .iter()
            .map(|l| SelectItem {
                label: l.to_string(),
                description: String::new(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            })
            .collect()
    }

    fn simulate_picker(
        items: Vec<SelectItem>,
        keys: &[KeyCode],
        multiselect: bool,
    ) -> Vec<SelectItem> {
        let mut state = crate::setup_tui::PickerState::new("Test", "", items, multiselect);
        for key in keys {
            if let Some(PickerResult::Confirmed(result)) = state.handle_key(*key) {
                return result;
            }
        }
        panic!("Key sequence did not produce a Confirmed result");
    }

    // ── API / data tests ────────────────────────────────────────

    #[test]
    fn test_workspace_api_ids_not_empty() {
        assert!(!WORKSPACE_APIS.is_empty());
    }

    #[test]
    fn test_workspace_api_ids_all_have_googleapis_suffix() {
        for api in WORKSPACE_APIS {
            assert!(
                api.id.ends_with(".googleapis.com"),
                "API ID '{}' should end with .googleapis.com",
                api.id
            );
        }
    }

    #[test]
    fn test_workspace_api_ids_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for api in WORKSPACE_APIS {
            assert!(seen.insert(api.id), "Duplicate API ID: {}", api.id);
        }
    }

    #[test]
    fn test_workspace_api_ids_covers_services() {
        let api_ids = all_api_ids();
        for entry in services::SERVICES {
            if entry.api_name == "modelarmor"
                || entry.api_name == "workspaceevents"
                || entry.api_name == "workflow"
            {
                continue;
            }
            let expected_suffix = if entry.api_name == "calendar" {
                "calendar-json.googleapis.com"
            } else {
                &format!("{}.googleapis.com", entry.api_name)
            };
            if entry.api_name == "admin" {
                assert!(api_ids.contains(&"admin.googleapis.com"));
                continue;
            }
            assert!(
                api_ids.iter().any(|id| *id == expected_suffix),
                "Missing API ID for service '{}' (expected {})",
                entry.api_name,
                expected_suffix
            );
        }
    }

    // ── parse_setup_args tests ──────────────────────────────────

    #[test]
    fn test_parse_setup_args_empty() {
        let opts = parse_setup_args(&[]);
        assert!(opts.project.is_none());
        assert!(!opts.dry_run);
    }

    #[test]
    fn test_parse_setup_args_with_project() {
        let args = vec!["--project".into(), "my-project".into()];
        let opts = parse_setup_args(&args);
        assert_eq!(opts.project.as_deref(), Some("my-project"));
    }

    #[test]
    fn test_parse_setup_args_with_project_equals() {
        let args = vec!["--project=my-project".into()];
        let opts = parse_setup_args(&args);
        assert_eq!(opts.project.as_deref(), Some("my-project"));
    }

    #[test]
    fn test_parse_setup_args_ignores_unknown() {
        let args = vec!["--verbose".into(), "--unknown".into()];
        let opts = parse_setup_args(&args);
        assert!(opts.project.is_none());
    }

    #[test]
    fn test_parse_setup_args_dry_run() {
        let args = vec!["--dry-run".into()];
        let opts = parse_setup_args(&args);
        assert!(opts.dry_run);
    }

    #[test]
    fn test_parse_setup_args_dry_run_with_project() {
        let args: Vec<String> = vec!["--dry-run".into(), "--project".into(), "p".into()];
        let opts = parse_setup_args(&args);
        assert!(opts.dry_run);
        assert_eq!(opts.project.as_deref(), Some("p"));
    }

    // ── Account selection → gcloud action ───────────────────────

    #[test]
    fn test_account_select_existing_triggers_set_account() {
        let mut items = make_items(&["➕ Login with new account", "user@gmail.com"]);
        items[1].selected = true;
        assert_eq!(
            resolve_account_selection(&items),
            SetupAction::SetAccount("user@gmail.com".into())
        );
    }

    #[test]
    fn test_account_select_login_new_triggers_login() {
        let mut items = make_items(&["➕ Login with new account", "user@gmail.com"]);
        items[0].selected = true;
        assert_eq!(
            resolve_account_selection(&items),
            SetupAction::LoginNewAccount
        );
    }

    #[test]
    fn test_account_select_none_returns_no_selection() {
        let items = make_items(&["➕ Login", "user@gmail.com"]);
        assert_eq!(resolve_account_selection(&items), SetupAction::NoSelection);
    }

    // ── Project selection → gcloud action ───────────────────────

    #[test]
    fn test_project_select_existing() {
        let mut items = make_items(&["➕ Create new project", "my-project-123"]);
        items[1].selected = true;
        assert_eq!(
            resolve_project_selection(&items),
            SetupAction::SetProject("my-project-123".into())
        );
    }

    #[test]
    fn test_project_select_create_new() {
        let mut items = make_items(&["➕ Create new project", "existing"]);
        items[0].selected = true;
        assert_eq!(
            resolve_project_selection(&items),
            SetupAction::CreateProject(String::new())
        );
    }

    #[test]
    fn test_project_select_none() {
        let items = make_items(&["➕ Create new project", "proj-a"]);
        assert_eq!(resolve_project_selection(&items), SetupAction::NoSelection);
    }

    // ── API selection → enable action ───────────────────────────

    #[test]
    fn test_api_select_none_enables_nothing() {
        let items: Vec<SelectItem> = WORKSPACE_APIS
            .iter()
            .map(|a| SelectItem {
                label: a.name.to_string(),
                description: a.id.to_string(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            })
            .collect();
        assert_eq!(
            resolve_api_selection(&items),
            SetupAction::EnableApis(vec![])
        );
    }

    #[test]
    fn test_api_select_first_enables_one() {
        let mut items: Vec<SelectItem> = WORKSPACE_APIS
            .iter()
            .map(|a| SelectItem {
                label: a.name.to_string(),
                description: a.id.to_string(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            })
            .collect();
        items[0].selected = true;
        assert_eq!(
            resolve_api_selection(&items),
            SetupAction::EnableApis(vec![WORKSPACE_APIS[0].id.to_string()])
        );
    }

    #[test]
    fn test_api_select_all_enables_all() {
        let items: Vec<SelectItem> = WORKSPACE_APIS
            .iter()
            .map(|a| SelectItem {
                label: a.name.to_string(),
                description: a.id.to_string(),
                selected: true,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            })
            .collect();
        match resolve_api_selection(&items) {
            SetupAction::EnableApis(ids) => assert_eq!(ids.len(), WORKSPACE_APIS.len()),
            _ => panic!("Expected EnableApis"),
        }
    }

    // ── Full pipeline: keys → picker → gcloud action ────────────

    #[test]
    fn test_pipeline_select_account_via_keys() {
        let items = vec![
            SelectItem {
                label: "➕ Login with new account".into(),
                description: String::new(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
            SelectItem {
                label: "user@gmail.com".into(),
                description: String::new(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
        ];
        let result = simulate_picker(
            items,
            &[KeyCode::Down, KeyCode::Char(' '), KeyCode::Enter],
            false,
        );
        assert_eq!(
            resolve_account_selection(&result),
            SetupAction::SetAccount("user@gmail.com".into())
        );
    }

    #[test]
    fn test_pipeline_login_new_via_keys() {
        let items = vec![
            SelectItem {
                label: "➕ Login with new account".into(),
                description: String::new(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
            SelectItem {
                label: "user@gmail.com".into(),
                description: String::new(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
        ];
        let result = simulate_picker(items, &[KeyCode::Char(' '), KeyCode::Enter], false);
        assert_eq!(
            resolve_account_selection(&result),
            SetupAction::LoginNewAccount
        );
    }

    #[test]
    fn test_pipeline_select_project_via_keys() {
        let items = vec![
            SelectItem {
                label: "➕ Create new project".into(),
                description: String::new(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
            SelectItem {
                label: "my-project".into(),
                description: "My Project".into(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
            SelectItem {
                label: "other-project".into(),
                description: "Other".into(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            },
        ];
        let result = simulate_picker(
            items,
            &[
                KeyCode::Down,
                KeyCode::Down,
                KeyCode::Char(' '),
                KeyCode::Enter,
            ],
            false,
        );
        assert_eq!(
            resolve_project_selection(&result),
            SetupAction::SetProject("other-project".into())
        );
    }

    #[test]
    fn test_pipeline_select_all_apis_via_keys() {
        let items: Vec<SelectItem> = WORKSPACE_APIS
            .iter()
            .map(|a| SelectItem {
                label: a.name.to_string(),
                description: a.id.to_string(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            })
            .collect();
        let result = simulate_picker(items, &[KeyCode::Char('a'), KeyCode::Enter], true);
        match resolve_api_selection(&result) {
            SetupAction::EnableApis(ids) => {
                assert_eq!(ids.len(), WORKSPACE_APIS.len());
                assert_eq!(ids[0], WORKSPACE_APIS[0].id);
            }
            _ => panic!("Expected EnableApis"),
        }
    }

    #[test]
    fn test_pipeline_select_two_apis_via_keys() {
        let items: Vec<SelectItem> = WORKSPACE_APIS
            .iter()
            .map(|a| SelectItem {
                label: a.name.to_string(),
                description: a.id.to_string(),
                selected: false,
                is_fixed: false,
                is_template: false,
                template_selects: vec![],
            })
            .collect();
        let result = simulate_picker(
            items,
            &[
                KeyCode::Char(' '),
                KeyCode::Down,
                KeyCode::Char(' '),
                KeyCode::Enter,
            ],
            true,
        );
        match resolve_api_selection(&result) {
            SetupAction::EnableApis(ids) => {
                assert_eq!(ids.len(), 2);
                assert_eq!(ids[0], WORKSPACE_APIS[0].id);
                assert_eq!(ids[1], WORKSPACE_APIS[1].id);
            }
            _ => panic!("Expected EnableApis"),
        }
    }
}

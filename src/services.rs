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

use crate::error::GwsError;

/// A known service with its alias, API name, version, and description.
pub struct ServiceEntry {
    pub aliases: &'static [&'static str],
    pub api_name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
}

/// All known services with metadata.
pub const SERVICES: &[ServiceEntry] = &[
    ServiceEntry {
        aliases: &["drive"],
        api_name: "drive",
        version: "v3",
        description: "Manage files, folders, and shared drives",
    },
    ServiceEntry {
        aliases: &["sheets"],
        api_name: "sheets",
        version: "v4",
        description: "Read and write spreadsheets",
    },
    ServiceEntry {
        aliases: &["gmail"],
        api_name: "gmail",
        version: "v1",
        description: "Send, read, and manage email",
    },
    ServiceEntry {
        aliases: &["calendar"],
        api_name: "calendar",
        version: "v3",
        description: "Manage calendars and events",
    },
    ServiceEntry {
        aliases: &["admin-reports", "reports"],
        api_name: "admin",
        version: "reports_v1",
        description: "Audit logs and usage reports",
    },
    ServiceEntry {
        aliases: &["docs"],
        api_name: "docs",
        version: "v1",
        description: "Read and write Google Docs",
    },
    ServiceEntry {
        aliases: &["slides"],
        api_name: "slides",
        version: "v1",
        description: "Read and write presentations",
    },
    ServiceEntry {
        aliases: &["tasks"],
        api_name: "tasks",
        version: "v1",
        description: "Manage task lists and tasks",
    },
    ServiceEntry {
        aliases: &["people"],
        api_name: "people",
        version: "v1",
        description: "Manage contacts and profiles",
    },
    ServiceEntry {
        aliases: &["chat"],
        api_name: "chat",
        version: "v1",
        description: "Manage Chat spaces and messages",
    },
    ServiceEntry {
        aliases: &["classroom"],
        api_name: "classroom",
        version: "v1",
        description: "Manage classes, rosters, and coursework",
    },
    ServiceEntry {
        aliases: &["forms"],
        api_name: "forms",
        version: "v1",
        description: "Read and write Google Forms",
    },
    ServiceEntry {
        aliases: &["keep"],
        api_name: "keep",
        version: "v1",
        description: "Manage Google Keep notes",
    },
    ServiceEntry {
        aliases: &["meet"],
        api_name: "meet",
        version: "v2",
        description: "Manage Google Meet conferences",
    },
    ServiceEntry {
        aliases: &["events"],
        api_name: "workspaceevents",
        version: "v1",
        description: "Subscribe to Google Workspace events",
    },
    ServiceEntry {
        aliases: &["modelarmor"],
        api_name: "modelarmor",
        version: "v1",
        description: "Filter user-generated content for safety",
    },
    ServiceEntry {
        aliases: &["workflow", "wf"],
        api_name: "workflow",
        version: "v1",
        description: "Cross-service productivity workflows",
    },
];

/// Selects the scope to request for an API method.
///
/// Google API methods list their accepted scopes from broadest to narrowest.
/// We pick only the first (broadest) scope because requesting multiple scopes
/// causes issues when restrictive scopes (e.g., `gmail.metadata`) are included,
/// as the API enforces that scope's restrictions even when broader scopes are
/// also present.
pub fn select_scope(scopes: &[String]) -> Option<&str> {
    scopes.first().map(|s| s.as_str())
}

/// Parses a service name (with optional `:version` suffix) and an `--api-version`
/// flag from raw CLI args into `(api_name, version)`.
pub fn parse_service_and_version(
    args: &[String],
    first_arg: &str,
) -> Result<(String, String), GwsError> {
    let mut service_arg = first_arg;
    let mut version_override: Option<String> = None;

    // Check for --api-version flag anywhere in args
    for i in 0..args.len() {
        if args[i] == "--api-version" && i + 1 < args.len() {
            version_override = Some(args[i + 1].clone());
        } else if let Some(val) = args[i].strip_prefix("--api-version=") {
            version_override = Some(val.to_string());
        }
    }

    // Support "service:version" syntax on the service arg itself
    if let Some((svc, ver)) = service_arg.split_once(':') {
        service_arg = svc;
        if version_override.is_none() {
            version_override = Some(ver.to_string());
        }
    }

    let (api_name, default_version) = resolve_service(service_arg)?;
    let version = version_override.unwrap_or(default_version);
    Ok((api_name, version))
}

/// Resolves a service alias to (api_name, version).
pub fn resolve_service(name: &str) -> Result<(String, String), GwsError> {
    for entry in SERVICES {
        if entry.aliases.contains(&name) {
            return Ok((entry.api_name.to_string(), entry.version.to_string()));
        }
    }
    let all_names: Vec<&str> = SERVICES
        .iter()
        .flat_map(|e| e.aliases.iter().copied())
        .collect();
    Err(GwsError::Validation(format!(
        "Unknown service '{}'. Known services: {}. Use '<api>:<version>' syntax for unlisted APIs.",
        name,
        all_names.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_service_known() {
        assert_eq!(
            resolve_service("drive").unwrap(),
            ("drive".to_string(), "v3".to_string())
        );
        assert_eq!(
            resolve_service("admin-reports").unwrap(),
            ("admin".to_string(), "reports_v1".to_string())
        );
        assert_eq!(
            resolve_service("reports").unwrap(),
            ("admin".to_string(), "reports_v1".to_string())
        );
    }

    #[test]
    fn test_select_scope_picks_first() {
        let scopes = vec![
            "https://mail.google.com/".to_string(),
            "https://www.googleapis.com/auth/gmail.metadata".to_string(),
            "https://www.googleapis.com/auth/gmail.modify".to_string(),
            "https://www.googleapis.com/auth/gmail.readonly".to_string(),
        ];
        assert_eq!(select_scope(&scopes), Some("https://mail.google.com/"));
    }

    #[test]
    fn test_select_scope_single() {
        let scopes = vec!["https://www.googleapis.com/auth/drive".to_string()];
        assert_eq!(
            select_scope(&scopes),
            Some("https://www.googleapis.com/auth/drive")
        );
    }

    #[test]
    fn test_select_scope_empty() {
        let scopes: Vec<String> = vec![];
        assert_eq!(select_scope(&scopes), None);
    }

    #[test]
    fn test_resolve_service_unknown() {
        let err = resolve_service("unknown_service");
        assert!(err.is_err());
        match err.unwrap_err() {
            GwsError::Validation(msg) => assert!(msg.contains("Unknown service")),
            _ => panic!("Expected Validation error"),
        }
    }
}

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

//! Google Workspace service registry.
//!
//! Maps service aliases (e.g. "drive", "gmail") to Discovery API names and versions.

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
    ServiceEntry {
        aliases: &["script"],
        api_name: "script",
        version: "v1",
        description: "Manage Google Apps Script projects",
    },
];

/// Splits a service spec of the form `<service>` or `<service>:<version>`.
///
/// Returns the service part and the explicit version, if one was given.
/// Nothing is validated here; see [`resolve_service_with_version`].
pub fn split_service_spec(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once(':') {
        Some((service, version)) => (service, Some(version)),
        None => (spec, None),
    }
}

/// Resolves a service name and an optional explicit version to
/// `(api_name, version)`.
///
/// - A registered alias resolves to its Discovery API name. An explicit
///   version overrides the registry's default version.
/// - A name that is not in the registry is treated as a raw Discovery API
///   name, but only when an explicit version is given (for example
///   `searchconsole:v1` or `analyticsdata:v1beta`). Both parts must pass
///   [`validate_api_identifier`](crate::validate::validate_api_identifier),
///   since they end up in cache filenames and discovery URLs.
/// - An unlisted name without a version is an error, so typos in a
///   registered alias still fail fast instead of fetching a missing API.
pub fn resolve_service_with_version(
    name: &str,
    explicit_version: Option<&str>,
) -> Result<(String, String), GwsError> {
    if let Some(entry) = SERVICES.iter().find(|e| e.aliases.contains(&name)) {
        let version = match explicit_version {
            Some(v) => crate::validate::validate_api_identifier(v)?,
            None => entry.version,
        };
        return Ok((entry.api_name.to_string(), version.to_string()));
    }

    match explicit_version {
        Some(version) => {
            let api_name = crate::validate::validate_api_identifier(name)?;
            let version = crate::validate::validate_api_identifier(version)?;
            Ok((api_name.to_string(), version.to_string()))
        }
        None => Err(unknown_service_error(name)),
    }
}

/// Resolves a service spec (`<alias>` or `<api>:<version>`) to
/// `(api_name, version)`.
pub fn resolve_service(spec: &str) -> Result<(String, String), GwsError> {
    let (name, version) = split_service_spec(spec);
    resolve_service_with_version(name, version)
}

fn unknown_service_error(name: &str) -> GwsError {
    let all_names: Vec<&str> = SERVICES
        .iter()
        .flat_map(|e| e.aliases.iter().copied())
        .collect();
    GwsError::Validation(format!(
        "Unknown service '{}'. Known services: {}. Use '<api>:<version>' syntax for unlisted APIs (e.g. 'searchconsole:v1').",
        name,
        all_names.join(", ")
    ))
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
    fn test_resolve_service_unknown() {
        let err = resolve_service("unknown_service");
        assert!(err.is_err());
        match err.unwrap_err() {
            GwsError::Validation(msg) => assert!(msg.contains("Unknown service")),
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_split_service_spec() {
        assert_eq!(split_service_spec("drive"), ("drive", None));
        assert_eq!(
            split_service_spec("searchconsole:v1"),
            ("searchconsole", Some("v1"))
        );
        assert_eq!(split_service_spec("drive:"), ("drive", Some("")));
    }

    #[test]
    fn test_resolve_service_unlisted_with_version() {
        assert_eq!(
            resolve_service("searchconsole:v1").unwrap(),
            ("searchconsole".to_string(), "v1".to_string())
        );
        assert_eq!(
            resolve_service("analyticsdata:v1beta").unwrap(),
            ("analyticsdata".to_string(), "v1beta".to_string())
        );
        assert_eq!(
            resolve_service_with_version("searchconsole", Some("v1")).unwrap(),
            ("searchconsole".to_string(), "v1".to_string())
        );
    }

    #[test]
    fn test_resolve_service_known_with_version_override() {
        assert_eq!(
            resolve_service("drive:v2").unwrap(),
            ("drive".to_string(), "v2".to_string())
        );
        // Aliases still map to their Discovery API name.
        assert_eq!(
            resolve_service_with_version("reports", Some("reports_v1")).unwrap(),
            ("admin".to_string(), "reports_v1".to_string())
        );
    }

    #[test]
    fn test_resolve_service_unlisted_without_version_errors() {
        match resolve_service("searchconsole").unwrap_err() {
            GwsError::Validation(msg) => {
                assert!(msg.contains("Unknown service 'searchconsole'"));
                assert!(msg.contains("<api>:<version>"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_resolve_service_rejects_unsafe_identifiers() {
        for spec in [
            "../etc:v1",
            "evil/api:v1",
            "searchconsole:../v1",
            "searchconsole:v1/../../x",
            "searchconsole:",
            ":v1",
            "drive:",
            "drive:v3?x=1",
            "api name:v1",
        ] {
            assert!(
                matches!(resolve_service(spec), Err(GwsError::Validation(_))),
                "expected {spec:?} to be rejected"
            );
        }
    }
}

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

use clap::{Arg, Command};

use crate::discovery::{RestDescription, RestResource};

/// Converts a camelCase string to kebab-case.
///
/// For example: `getProfile` → `get-profile`, `calendarList` → `calendar-list`.
/// If the name is already lowercase (no uppercase letters), the original string
/// is returned unchanged so we don't register a no-op alias.
pub fn camel_to_kebab(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(ch.to_ascii_lowercase());
    }
    result
}

/// Converts a kebab-case string to camelCase.
///
/// For example: `get-profile` → `getProfile`, `calendar-list` → `calendarList`.
pub fn kebab_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Resolves a map key given either its exact name or a kebab-case spelling.
///
/// Clap normally returns the canonical command name when an alias matches,
/// so this helper is used as a defense-in-depth measure inside
/// `resolve_method_from_matches`.
pub fn resolve_name<'a>(
    mut map_keys: impl Iterator<Item = &'a String>,
    input: &str,
) -> Option<String> {
    let camel = kebab_to_camel(input);
    map_keys
        .find(|k| k.as_str() == input || k.as_str() == camel)
        .cloned()
}

/// Builds the full CLI command tree from a Discovery Document.
pub fn build_cli(doc: &RestDescription) -> Command {
    let about_text = doc
        .description
        .clone()
        .unwrap_or_else(|| "Google Workspace CLI".to_string());
    let mut root = Command::new("gws")
        .about(about_text)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            clap::Arg::new("sanitize")
                .long("sanitize")
                .help("Sanitize API responses through a Model Armor template. Requires cloud-platform scope. Format: projects/PROJECT/locations/LOCATION/templates/TEMPLATE. Also reads GWS_SANITIZE_TEMPLATE env var.")
                .value_name("TEMPLATE")
                .global(true),
        )
        .arg(
            clap::Arg::new("dry-run")
                .long("dry-run")
                .help("Validate the request locally without sending it to the API")
                .action(clap::ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            clap::Arg::new("format")
                .long("format")
                .help("Output format: json (default), table, yaml, csv")
                .value_name("FORMAT")
                .global(true),
        );

    // Inject helper commands
    let helper = crate::helpers::get_helper(&doc.name);
    if let Some(ref helper) = helper {
        root = helper.inject_commands(root, doc);
    }

    // Add resource subcommands (unless helper suppresses them)
    let skip_resources = helper.as_ref().is_some_and(|h| h.helper_only());
    if !skip_resources {
        let mut resource_names: Vec<_> = doc.resources.keys().collect();
        resource_names.sort();
        for name in resource_names {
            let resource = &doc.resources[name];
            if let Some(cmd) = build_resource_command(name, resource) {
                root = root.subcommand(cmd);
            }
        }
    }

    root
}

/// Recursively builds a Command for a resource.
///
/// Every command whose name contains uppercase letters gets a visible
/// kebab-case alias registered automatically.  For example, the resource
/// `calendarList` also accepts `calendar-list`, and the method `getProfile`
/// also accepts `get-profile`.  Existing camelCase names continue to work
/// unchanged.
///
/// Returns None if the resource has no methods or sub-resources.
fn build_resource_command(name: &str, resource: &RestResource) -> Option<Command> {
    let kebab = camel_to_kebab(name);
    let mut cmd = Command::new(name.to_string())
        .about(format!("Operations on the '{name}' resource"))
        .subcommand_required(true)
        .arg_required_else_help(true);

    // Register visible kebab-case alias only when the name actually differs.
    if kebab != name {
        cmd = cmd.visible_alias(kebab);
    }

    let mut has_children = false;

    // Add method subcommands
    let mut method_names: Vec<_> = resource.methods.keys().collect();
    method_names.sort();
    for method_name in method_names {
        let method = &resource.methods[method_name];

        has_children = true;

        let about = method
            .description
            .as_deref()
            .unwrap_or("")
            // Truncate long descriptions for help text
            .chars()
            .take(200)
            .collect::<String>();

        let method_kebab = camel_to_kebab(method_name);

        let mut method_cmd = Command::new(method_name.to_string())
            .about(about)
            .arg(
                Arg::new("params")
                    .long("params")
                    .help("JSON string for URL/Query parameters")
                    .value_name("JSON"),
            )
            .arg(
                Arg::new("output")
                    .long("output")
                    .short('o')
                    .help("Output file path for binary responses")
                    .value_name("PATH"),
            );

        // Register visible kebab-case alias only when the name actually differs.
        if method_kebab != *method_name {
            method_cmd = method_cmd.visible_alias(method_kebab);
        }

        // Only add --json flag if the method accepts a request body
        if method.request.is_some() {
            method_cmd = method_cmd.arg(
                Arg::new("json")
                    .long("json")
                    .help("JSON string for the request body")
                    .value_name("JSON"),
            );
        }

        // Add --upload flag if the method supports media upload
        if method.supports_media_upload {
            method_cmd = method_cmd.arg(
                Arg::new("upload")
                    .long("upload")
                    .help("Local file path to upload as media content (multipart upload)")
                    .value_name("PATH"),
            );
        }

        // Pagination flags
        method_cmd = method_cmd
            .arg(
                Arg::new("page-all")
                    .long("page-all")
                    .help("Auto-paginate through all results, outputting one JSON line per page (NDJSON)")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                Arg::new("page-limit")
                    .long("page-limit")
                    .help("Maximum number of pages to fetch when using --page-all (default: 10)")
                    .value_name("N")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                Arg::new("page-delay")
                    .long("page-delay")
                    .help("Delay in milliseconds between page fetches (default: 100)")
                    .value_name("MS")
                    .value_parser(clap::value_parser!(u64)),
            );

        cmd = cmd.subcommand(method_cmd);
    }

    // Add sub-resource subcommands (recursive)
    let mut sub_names: Vec<_> = resource.resources.keys().collect();
    sub_names.sort();
    for sub_name in sub_names {
        let sub_resource = &resource.resources[sub_name];
        if let Some(sub_cmd) = build_resource_command(sub_name, sub_resource) {
            has_children = true;
            cmd = cmd.subcommand(sub_cmd);
        }
    }

    if has_children {
        Some(cmd)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{RestMethod, RestResource};
    use std::collections::HashMap;

    fn make_doc() -> RestDescription {
        let mut methods = HashMap::new();
        methods.insert(
            "list".to_string(),
            RestMethod {
                id: None,
                description: None,
                http_method: "GET".to_string(),
                path: "list".to_string(),
                parameters: HashMap::new(),
                parameter_order: vec![],
                request: None,
                response: None,
                scopes: vec!["https://www.googleapis.com/auth/drive.readonly".to_string()],
                flat_path: None,
                supports_media_download: false,
                supports_media_upload: false,
                media_upload: None,
            },
        );

        methods.insert(
            "delete".to_string(),
            RestMethod {
                id: None,
                description: None,
                http_method: "DELETE".to_string(),
                path: "delete".to_string(),
                parameters: HashMap::new(),
                parameter_order: vec![],
                request: None,
                response: None,
                scopes: vec!["https://www.googleapis.com/auth/drive".to_string()],
                flat_path: None,
                supports_media_download: false,
                supports_media_upload: false,
                media_upload: None,
            },
        );

        let mut resources = HashMap::new();
        resources.insert(
            "files".to_string(),
            RestResource {
                methods,
                resources: HashMap::new(),
            },
        );

        RestDescription {
            name: "drive".to_string(),
            version: "v3".to_string(),
            title: None,
            description: None,
            root_url: "".to_string(),
            service_path: "".to_string(),
            base_url: None,
            schemas: HashMap::new(),
            resources,
            parameters: HashMap::new(),
            auth: None,
        }
    }

    #[test]
    fn test_all_commands_always_shown() {
        let doc = make_doc();
        let cmd = build_cli(&doc);

        // Should have "files" subcommand
        let files_cmd = cmd
            .find_subcommand("files")
            .expect("files resource missing");

        // All methods should always be visible regardless of auth state
        assert!(files_cmd.find_subcommand("list").is_some());
        assert!(files_cmd.find_subcommand("delete").is_some());
    }

    #[test]
    fn test_sanitize_arg_present() {
        let doc = make_doc();
        let cmd = build_cli(&doc);

        // The --sanitize global arg should be available
        let args: Vec<_> = cmd.get_arguments().collect();
        let sanitize_arg = args.iter().find(|a| a.get_id() == "sanitize");
        assert!(
            sanitize_arg.is_some(),
            "--sanitize arg should be present on root command"
        );
    }

    // ── kebab-case utility tests ──────────────────────────────────────────────

    #[test]
    fn test_camel_to_kebab_single_word() {
        assert_eq!(camel_to_kebab("list"), "list");
        assert_eq!(camel_to_kebab("delete"), "delete");
    }

    #[test]
    fn test_camel_to_kebab_multi_word() {
        assert_eq!(camel_to_kebab("getProfile"), "get-profile");
        assert_eq!(camel_to_kebab("calendarList"), "calendar-list");
        assert_eq!(camel_to_kebab("sendAs"), "send-as");
    }

    #[test]
    fn test_kebab_to_camel_single_word() {
        assert_eq!(kebab_to_camel("list"), "list");
        assert_eq!(kebab_to_camel("delete"), "delete");
    }

    #[test]
    fn test_kebab_to_camel_multi_word() {
        assert_eq!(kebab_to_camel("get-profile"), "getProfile");
        assert_eq!(kebab_to_camel("calendar-list"), "calendarList");
        assert_eq!(kebab_to_camel("send-as"), "sendAs");
    }

    #[test]
    fn test_camel_kebab_roundtrip() {
        for original in ["getProfile", "calendarList", "sendAs", "insertMedia"] {
            let kebab = camel_to_kebab(original);
            let back = kebab_to_camel(&kebab);
            assert_eq!(back, original, "roundtrip failed for '{original}'");
        }
    }

    // ── resolve_name helper tests ─────────────────────────────────────────────

    #[test]
    fn test_resolve_name_exact() {
        let keys: Vec<String> = vec!["getProfile".to_string(), "list".to_string()];
        assert_eq!(
            resolve_name(keys.iter(), "getProfile"),
            Some("getProfile".to_string())
        );
    }

    #[test]
    fn test_resolve_name_kebab() {
        let keys: Vec<String> = vec!["getProfile".to_string(), "list".to_string()];
        assert_eq!(
            resolve_name(keys.iter(), "get-profile"),
            Some("getProfile".to_string())
        );
    }

    #[test]
    fn test_resolve_name_not_found() {
        let keys: Vec<String> = vec!["getProfile".to_string()];
        assert_eq!(resolve_name(keys.iter(), "nonexistent"), None);
    }

    // ── Gmail users getProfile alias tests ───────────────────────────────────

    fn make_gmail_doc() -> RestDescription {
        let mut users_methods = HashMap::new();
        users_methods.insert(
            "getProfile".to_string(),
            RestMethod {
                id: Some("gmail.users.getProfile".to_string()),
                description: Some("Gets the current user's Gmail profile.".to_string()),
                http_method: "GET".to_string(),
                path: "gmail/v1/users/{userId}/profile".to_string(),
                parameters: HashMap::new(),
                parameter_order: vec![],
                request: None,
                response: None,
                scopes: vec!["https://www.googleapis.com/auth/gmail.readonly".to_string()],
                flat_path: None,
                supports_media_download: false,
                supports_media_upload: false,
                media_upload: None,
            },
        );

        let mut resources = HashMap::new();
        resources.insert(
            "users".to_string(),
            RestResource {
                methods: users_methods,
                resources: HashMap::new(),
            },
        );

        RestDescription {
            name: "gmail".to_string(),
            version: "v1".to_string(),
            title: None,
            description: None,
            root_url: "".to_string(),
            service_path: "".to_string(),
            base_url: None,
            schemas: HashMap::new(),
            resources,
            parameters: HashMap::new(),
            auth: None,
        }
    }

    /// Gmail users getProfile — camelCase canonical name is found by clap.
    #[test]
    fn test_gmail_users_get_profile_canonical() {
        let doc = make_gmail_doc();
        let cmd = build_cli(&doc);
        let users_cmd = cmd
            .find_subcommand("users")
            .expect("users resource missing");
        assert!(
            users_cmd.find_subcommand("getProfile").is_some(),
            "camelCase 'getProfile' should be a registered subcommand"
        );
    }

    /// Gmail users getProfile — kebab-case alias is accepted by clap.
    #[test]
    fn test_gmail_users_get_profile_kebab_alias() {
        let doc = make_gmail_doc();
        let cmd = build_cli(&doc);
        let users_cmd = cmd
            .find_subcommand("users")
            .expect("users resource missing");
        assert!(
            users_cmd.find_subcommand("get-profile").is_some(),
            "kebab-case alias 'get-profile' should resolve via clap"
        );
    }

    /// Gmail users getProfile — ArgMatches from kebab input returns canonical name.
    #[test]
    fn test_gmail_users_get_profile_kebab_matches_canonical() {
        let doc = make_gmail_doc();
        let cmd = build_cli(&doc);

        let matches = cmd
            .clone()
            .try_get_matches_from(["gws", "users", "get-profile"])
            .expect("clap should accept kebab-case alias");

        let (name, sub) = matches.subcommand().expect("should have a subcommand");
        assert_eq!(name, "users");
        let (method_name, _) = sub.subcommand().expect("should have a method subcommand");
        assert_eq!(
            method_name, "getProfile",
            "clap should resolve alias to canonical name 'getProfile'"
        );
    }

    // ── Calendar calendarList alias tests ────────────────────────────────────

    fn make_calendar_doc() -> RestDescription {
        let mut calendar_list_methods = HashMap::new();
        calendar_list_methods.insert(
            "list".to_string(),
            RestMethod {
                id: Some("calendar.calendarList.list".to_string()),
                description: Some("Returns the calendars on the user's calendar list.".to_string()),
                http_method: "GET".to_string(),
                path: "users/me/calendarList".to_string(),
                parameters: HashMap::new(),
                parameter_order: vec![],
                request: None,
                response: None,
                scopes: vec!["https://www.googleapis.com/auth/calendar.readonly".to_string()],
                flat_path: None,
                supports_media_download: false,
                supports_media_upload: false,
                media_upload: None,
            },
        );

        let mut resources = HashMap::new();
        resources.insert(
            "calendarList".to_string(),
            RestResource {
                methods: calendar_list_methods,
                resources: HashMap::new(),
            },
        );

        RestDescription {
            name: "calendar".to_string(),
            version: "v3".to_string(),
            title: None,
            description: None,
            root_url: "".to_string(),
            service_path: "".to_string(),
            base_url: None,
            schemas: HashMap::new(),
            resources,
            parameters: HashMap::new(),
            auth: None,
        }
    }

    /// Calendar calendarList — camelCase resource name is found.
    #[test]
    fn test_calendar_calendar_list_canonical() {
        let doc = make_calendar_doc();
        let cmd = build_cli(&doc);
        assert!(
            cmd.find_subcommand("calendarList").is_some(),
            "camelCase resource 'calendarList' should be present"
        );
    }

    /// Calendar calendarList — kebab-case alias is accepted.
    #[test]
    fn test_calendar_calendar_list_kebab_alias() {
        let doc = make_calendar_doc();
        let cmd = build_cli(&doc);
        assert!(
            cmd.find_subcommand("calendar-list").is_some(),
            "kebab-case alias 'calendar-list' should resolve to 'calendarList'"
        );
    }

    /// Calendar calendarList — full invocation via kebab-case resolves to canonical names.
    #[test]
    fn test_calendar_calendar_list_kebab_matches_canonical() {
        let doc = make_calendar_doc();
        let cmd = build_cli(&doc);

        let matches = cmd
            .clone()
            .try_get_matches_from(["gws", "calendar-list", "list"])
            .expect("clap should accept kebab-case resource alias");

        let (resource_name, sub) = matches.subcommand().expect("should have a subcommand");
        assert_eq!(
            resource_name, "calendarList",
            "clap should resolve 'calendar-list' alias to canonical 'calendarList'"
        );
        let (method_name, _) = sub.subcommand().expect("should have method subcommand");
        assert_eq!(method_name, "list");
    }
}

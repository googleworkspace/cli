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

//! Google Workspace CLI (gws)
//!
//! A dynamic, schema-driven CLI for Google Workspace APIs.
//! This tool dynamically parses Google API Discovery Documents to construct CLI commands.
//! It supports deep schema validation, OAuth / Service Account authentication,
//! interactive prompts, and integration with Model Armor.

mod auth;
pub(crate) mod auth_commands;
mod client;
mod commands;
pub(crate) mod credential_store;
mod discovery;
mod error;
mod executor;
mod formatter;
mod fs_util;
mod generate_skills;
mod helpers;
mod oauth_config;
mod schema;
mod services;
mod setup;
mod setup_tui;
mod text;
mod token_storage;
pub(crate) mod validate;

use error::{print_error_json, GwsError};

#[tokio::main]
async fn main() {
    // Load .env file if present (silently ignored if missing)
    let _ = dotenvy::dotenv();

    if let Err(err) = run().await {
        print_error_json(&err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), GwsError> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Err(GwsError::Validation(
            "No service specified. Usage: gws <service> <resource> [sub-resource] <method> [flags]"
                .to_string(),
        ));
    }

    let mut filtered_args = vec![args[0].clone()];
    let mut first_arg: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        
        if a == "--profile" {
            i += 2; // Skip --profile and its value
            continue;
        }
        if a.starts_with("--profile=") {
            i += 1;
            continue;
        }

        filtered_args.push(a.clone());
        
        if a == "--api-version" {
            if i + 1 < args.len() {
                filtered_args.push(args[i+1].clone());
            }
            i += 2; // Skip both the flag and the value
            continue;
        }
        if a.starts_with("--api-version=") {
            i += 1;
            continue;
        }
        
        if first_arg.is_none() && !a.starts_with("--") && a != "--help" && a != "--version" {
            first_arg = Some(a.clone());
        }
        i += 1;
    }

    // Extract profile early to set environment variable
    let mut profile_name = None;
    for i in 1..args.len() {
        if args[i] == "--profile" && i + 1 < args.len() {
            profile_name = Some(args[i + 1].clone());
            break;
        } else if let Some(stripped) = args[i].strip_prefix("--profile=") {
            profile_name = Some(stripped.to_string());
            break;
        }
    }

    if let Some(profile) = profile_name {
        std::env::set_var("GOOGLE_WORKSPACE_CLI_PROFILE", profile);
    }

    // Use filtered_args for the rest of path resolution (helps `filter_args_for_subcommand`)
    let args = filtered_args;

    let first_arg = first_arg.ok_or_else(|| {
        GwsError::Validation(
            "No service specified. Usage: gws <service> <resource> [sub-resource] <method> [flags]"
                .to_string(),
        )
    })?;

    // Handle --help and --version at top level
    if is_help_flag(&first_arg) {
        print_usage();
        return Ok(());
    }

    if is_version_flag(&first_arg) {
        println!("gws {}", env!("CARGO_PKG_VERSION"));
        println!("This is not an officially supported Google product.");
        return Ok(());
    }

    // Handle the `schema` command
    if first_arg == "schema" {
        if args.len() < 3 {
            return Err(GwsError::Validation(
                "Usage: gws schema <service.resource.method> (e.g., gws schema drive.files.list) [--resolve-refs]"
                    .to_string(),
            ));
        }
        let resolve_refs = args.iter().any(|arg| arg == "--resolve-refs");
        // Remove the flag if it exists so it doesn't mess up path parsing, or just pass the path
        // The path is args[2], flags might follow.
        let path = &args[2];
        return schema::handle_schema_command(path, resolve_refs).await;
    }

    // Handle the `generate-skills` command
    if first_arg == "generate-skills" {
        let gen_args: Vec<String> = args.iter().skip(2).cloned().collect();
        return generate_skills::handle_generate_skills(&gen_args).await;
    }

    // Handle the `auth` command
    if first_arg == "auth" {
        let auth_args: Vec<String> = args.iter().skip(2).cloned().collect();
        return auth_commands::handle_auth_command(&auth_args).await;
    }

    // Parse service name and optional version override
    let (api_name, version) = parse_service_and_version(&args, &first_arg)?;

    // For synthetic services (no Discovery doc), use an empty RestDescription
    let doc = if api_name == "workflow" {
        discovery::RestDescription {
            name: "workflow".to_string(),
            description: Some("Cross-service productivity workflows".to_string()),
            ..Default::default()
        }
    } else {
        // Fetch the Discovery Document
        discovery::fetch_discovery_document(&api_name, &version)
            .await
            .map_err(|e| GwsError::Discovery(format!("{e:#}")))?
    };

    // Build the dynamic command tree (all commands shown regardless of auth state)
    let cli = commands::build_cli(&doc);

    // Re-parse args (skip argv[0] which is the binary, and argv[1] which is the service name)
    // Filter out --api-version and its value
    // Prepend "gws" as the program name since try_get_matches_from expects argv[0]
    let sub_args = filter_args_for_subcommand(&args, &first_arg);

    let matches = cli.try_get_matches_from(&sub_args).map_err(|e| {
        // If it's a help or version display, print it and exit cleanly
        if e.kind() == clap::error::ErrorKind::DisplayHelp
            || e.kind() == clap::error::ErrorKind::DisplayVersion
        {
            print!("{e}");
            std::process::exit(0);
        }
        GwsError::Validation(e.to_string())
    })?;

    // Resolve --format flag
    let output_format = match matches.get_one::<String>("format") {
        Some(s) => match formatter::OutputFormat::parse(s) {
            Ok(fmt) => fmt,
            Err(unknown) => {
                eprintln!(
                    "warning: unknown output format '{unknown}'; falling back to json (valid options: json, table, yaml, csv)"
                );
                formatter::OutputFormat::Json
            }
        },
        None => formatter::OutputFormat::default(),
    };

    // Resolve --sanitize template (flag or env var)
    let sanitize_template = matches
        .get_one::<String>("sanitize")
        .cloned()
        .or_else(|| std::env::var("GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE").ok());

    let sanitize_mode = std::env::var("GOOGLE_WORKSPACE_CLI_SANITIZE_MODE")
        .map(|v| helpers::modelarmor::SanitizeMode::from_str(&v))
        .unwrap_or(helpers::modelarmor::SanitizeMode::Warn);

    let sanitize_config = parse_sanitize_config(sanitize_template, &sanitize_mode)?;

    // Check if a helper wants to handle this command
    if let Some(helper) = helpers::get_helper(&doc.name) {
        if helper.handle(&doc, &matches, &sanitize_config).await? {
            return Ok(());
        }
    }

    // Walk the subcommand tree to find the target method
    let (method, matched_args) = resolve_method_from_matches(&doc, &matches)?;

    let params_json = matched_args.get_one::<String>("params").map(|s| s.as_str());
    let body_json = matched_args
        .try_get_one::<String>("json")
        .ok()
        .flatten()
        .map(|s| s.as_str());
    let output_path = matched_args.get_one::<String>("output").map(|s| s.as_str());
    let upload_path = matched_args
        .try_get_one::<String>("upload")
        .ok()
        .flatten()
        .map(|s| s.as_str());

    let dry_run = matched_args.get_flag("dry-run");

    // Build pagination config from flags
    let pagination = parse_pagination_config(matched_args);

    // Select the best scope for the method. Discovery Documents list scopes as
    // alternatives (any one grants access). We pick the first (broadest) scope
    // to avoid restrictive scopes like gmail.metadata that block query parameters.
    let scopes: Vec<&str> = select_scope(&method.scopes).into_iter().collect();

    // Authenticate: try OAuth, fail with error if credentials exist but are broken
    let (token, auth_method) = match auth::get_token(&scopes).await {
        Ok(t) => (Some(t), executor::AuthMethod::OAuth),
        Err(e) => {
            // If credentials were found but failed (e.g. decryption error, invalid token),
            // propagate the error instead of silently falling back to unauthenticated.
            // Only fall back to None if no credentials exist at all.
            let err_msg = format!("{e:#}");
            // NB: matches the bail!() message in auth::load_credentials_inner
            if err_msg.starts_with("No credentials found") {
                (None, executor::AuthMethod::None)
            } else {
                return Err(GwsError::Auth(format!("Authentication failed: {err_msg}")));
            }
        }
    };

    // Execute
    executor::execute_method(
        &doc,
        method,
        params_json,
        body_json,
        token.as_deref(),
        auth_method,
        output_path,
        upload_path,
        dry_run,
        &pagination,
        sanitize_config.template.as_deref(),
        &sanitize_config.mode,
        &output_format,
        false,
    )
    .await
    .map(|_| ())
}

/// Select the best scope from a method's scope list.
///
/// Discovery Documents list method scopes as alternatives — any single scope
/// grants access. The first scope is typically the broadest. Using all scopes
/// causes issues when restrictive scopes (e.g., `gmail.metadata`) are included,
/// as the API enforces that scope's restrictions even when broader scopes are
/// also present.
pub(crate) fn select_scope(scopes: &[String]) -> Option<&str> {
    scopes.first().map(|s| s.as_str())
}

fn parse_pagination_config(matches: &clap::ArgMatches) -> executor::PaginationConfig {
    executor::PaginationConfig {
        page_all: matches.get_flag("page-all"),
        page_limit: matches.get_one::<u32>("page-limit").copied().unwrap_or(10),
        page_delay_ms: matches.get_one::<u64>("page-delay").copied().unwrap_or(100),
    }
}

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
        }
    }

    // Support "service:version" syntax on the service arg itself
    if let Some((svc, ver)) = service_arg.split_once(':') {
        service_arg = svc;
        if version_override.is_none() {
            version_override = Some(ver.to_string());
        }
    }

    let (api_name, default_version) = services::resolve_service(service_arg)?;
    let version = version_override.unwrap_or(default_version);
    Ok((api_name, version))
}

pub fn filter_args_for_subcommand(args: &[String], service_name: &str) -> Vec<String> {
    let mut sub_args: Vec<String> = vec!["gws".to_string()];
    let mut skip_next = false;
    let mut service_skipped = false;
    for arg in args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--api-version" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("--api-version=") {
            continue;
        }
        if !service_skipped && arg == service_name {
            service_skipped = true;
            continue;
        }
        sub_args.push(arg.clone());
    }
    sub_args
}

fn parse_sanitize_config(
    template: Option<String>,
    mode: &helpers::modelarmor::SanitizeMode,
) -> Result<helpers::modelarmor::SanitizeConfig, GwsError> {
    Ok(helpers::modelarmor::SanitizeConfig {
        template,
        mode: mode.clone(),
    })
}

/// Recursively walks clap ArgMatches to find the leaf method and its matches.
fn resolve_method_from_matches<'a>(
    doc: &'a discovery::RestDescription,
    matches: &'a clap::ArgMatches,
) -> Result<(&'a discovery::RestMethod, &'a clap::ArgMatches), GwsError> {
    // Walk the subcommand chain
    let mut path: Vec<&str> = Vec::new();
    let mut current_matches = matches;

    while let Some((sub_name, sub_matches)) = current_matches.subcommand() {
        path.push(sub_name);
        current_matches = sub_matches;
    }

    if path.is_empty() {
        return Err(GwsError::Validation(
            "No resource or method specified".to_string(),
        ));
    }

    // path looks like ["files", "list"] or ["files", "permissions", "list"]
    // Walk the Discovery Document resources to find the method
    let resource_name = path[0];
    let resource = doc
        .resources
        .get(resource_name)
        .ok_or_else(|| GwsError::Validation(format!("Resource '{resource_name}' not found")))?;

    let mut current_resource = resource;

    // Navigate sub-resources (everything except the last element, which is the method)
    for &name in &path[1..path.len() - 1] {
        // Check if this is a sub-resource
        if let Some(sub) = current_resource.resources.get(name) {
            current_resource = sub;
        } else {
            return Err(GwsError::Validation(format!(
                "Sub-resource '{name}' not found"
            )));
        }
    }

    // The last element is the method name
    let method_name = path[path.len() - 1];

    // Check if this is a method on the current resource
    if let Some(method) = current_resource.methods.get(method_name) {
        return Ok((method, current_matches));
    }

    // Maybe it's a resource that has methods — need one more subcommand
    Err(GwsError::Validation(format!(
        "Method '{method_name}' not found on resource. Available methods: {:?}",
        current_resource.methods.keys().collect::<Vec<_>>()
    )))
}

fn print_usage() {
    println!("gws — Google Workspace CLI");
    println!();
    println!("USAGE:");
    println!("    gws <service> <resource> [sub-resource] <method> [flags]");
    println!("    gws schema <service.resource.method> [--resolve-refs]");
    println!();
    println!("EXAMPLES:");
    println!("    gws drive files list --params '{{\"pageSize\": 10}}'");
    println!("    gws drive files get --params '{{\"fileId\": \"abc123\"}}'");
    println!("    gws sheets spreadsheets get --params '{{\"spreadsheetId\": \"...\"}}'");
    println!("    gws gmail users messages list --params '{{\"userId\": \"me\"}}'");
    println!("    gws schema drive.files.list");
    println!();
    println!("FLAGS:");
    println!("    --params <JSON>       URL/Query parameters as JSON");
    println!("    --json <JSON>         Request body as JSON (POST/PATCH/PUT)");
    println!("    --upload <PATH>       Local file to upload as media content (multipart)");
    println!("    --output <PATH>       Output file path for binary responses");
    println!("    --format <FMT>        Output format: json (default), table, yaml, csv");
    println!("    --profile <NAME>      Use a specific configuration profile");
    println!("    --api-version <VER>   Override the API version (e.g., v2, v3)");
    println!("    --page-all            Auto-paginate, one JSON line per page (NDJSON)");
    println!("    --page-limit <N>      Max pages to fetch with --page-all (default: 10)");
    println!("    --page-delay <MS>     Delay between pages in ms (default: 100)");
    println!();
    println!("SERVICES:");
    for entry in services::SERVICES {
        let name = entry.aliases[0];
        let aliases = if entry.aliases.len() > 1 {
            format!(" (also: {})", entry.aliases[1..].join(", "))
        } else {
            String::new()
        };
        println!("    {:<20} {}{}", name, entry.description, aliases);
    }
    println!();
    println!("ENVIRONMENT:");
    println!("    GOOGLE_WORKSPACE_CLI_TOKEN               Pre-obtained OAuth2 access token (highest priority)");
    println!("    GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE    Path to OAuth credentials JSON file");
    println!("    GOOGLE_WORKSPACE_CLI_CLIENT_ID           OAuth client ID (for gws auth login)");
    println!(
        "    GOOGLE_WORKSPACE_CLI_CLIENT_SECRET       OAuth client secret (for gws auth login)"
    );
    println!(
        "    GOOGLE_WORKSPACE_CLI_CONFIG_DIR          Override config directory (default: ~/.config/gws)"
    );
    println!("    GOOGLE_WORKSPACE_CLI_PROFILE             Configuration profile to use (e.g., 'work', 'personal')");
    println!("    GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE   Default Model Armor template");
    println!(
        "    GOOGLE_WORKSPACE_CLI_SANITIZE_MODE       Sanitization mode: warn (default) or block"
    );
    println!(
        "    GOOGLE_WORKSPACE_PROJECT_ID              GCP project ID fallback for helper commands"
    );
    println!();
    println!("COMMUNITY:");
    println!("    Star the repo: https://github.com/googleworkspace/cli");
    println!("    Report bugs / request features: https://github.com/googleworkspace/cli/issues");
    println!("    Please search existing issues first; if one already exists, comment there.");
    println!();
    println!("DISCLAIMER:");
    println!("    This is not an officially supported Google product.");
}

fn is_help_flag(arg: &str) -> bool {
    matches!(arg, "--help" | "-h")
}

fn is_version_flag(arg: &str) -> bool {
    matches!(arg, "--version" | "-V" | "version")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pagination_config_defaults() {
        let matches = clap::Command::new("test")
            .arg(
                clap::Arg::new("page-all")
                    .long("page-all")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                clap::Arg::new("page-limit")
                    .long("page-limit")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                clap::Arg::new("page-delay")
                    .long("page-delay")
                    .value_parser(clap::value_parser!(u64)),
            )
            .get_matches_from(vec!["test"]);

        let config = parse_pagination_config(&matches);
        assert_eq!(config.page_all, false);
        assert_eq!(config.page_limit, 10);
        assert_eq!(config.page_delay_ms, 100);
    }

    #[test]
    fn test_parse_pagination_config_custom() {
        let matches = clap::Command::new("test")
            .arg(
                clap::Arg::new("page-all")
                    .long("page-all")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                clap::Arg::new("page-limit")
                    .long("page-limit")
                    .value_parser(clap::value_parser!(u32)),
            )
            .arg(
                clap::Arg::new("page-delay")
                    .long("page-delay")
                    .value_parser(clap::value_parser!(u64)),
            )
            .get_matches_from(vec![
                "test",
                "--page-all",
                "--page-limit",
                "20",
                "--page-delay",
                "500",
            ]);

        let config = parse_pagination_config(&matches);
        assert_eq!(config.page_all, true);
        assert_eq!(config.page_limit, 20);
        assert_eq!(config.page_delay_ms, 500);
    }

    #[test]
    fn test_parse_sanitize_config_valid() {
        let config = parse_sanitize_config(
            Some("tpl".to_string()),
            &helpers::modelarmor::SanitizeMode::Warn,
        )
        .unwrap();
        assert_eq!(config.template.as_deref(), Some("tpl"));
    }

    #[test]
    fn test_parse_sanitize_config_no_template() {
        let config =
            parse_sanitize_config(None, &helpers::modelarmor::SanitizeMode::Block).unwrap();
        assert!(config.template.is_none());
        assert_eq!(config.mode, helpers::modelarmor::SanitizeMode::Block);
    }

    #[test]
    fn test_is_version_flag() {
        assert!(is_version_flag("--version"));
        assert!(is_version_flag("-V"));
        assert!(is_version_flag("version"));
        assert!(!is_version_flag("--ver"));
        assert!(!is_version_flag("v"));
        assert!(!is_version_flag("drive"));
    }

    #[test]
    fn test_is_help_flag() {
        assert!(is_help_flag("--help"));
        assert!(is_help_flag("-h"));
        assert!(!is_help_flag("help"));
        assert!(!is_help_flag("--h"));
    }

    #[test]
    fn test_resolve_method_from_matches_basic() {
        let mut resources = std::collections::HashMap::new();
        let mut files_res = crate::discovery::RestResource::default();
        files_res.methods.insert(
            "list".to_string(),
            crate::discovery::RestMethod {
                id: Some("drive.files.list".to_string()),
                http_method: "GET".to_string(),
                ..Default::default()
            },
        );
        resources.insert("files".to_string(), files_res);

        let doc = discovery::RestDescription {
            name: "drive".to_string(),
            resources,
            ..Default::default()
        };

        // Simulate CLI structure
        let cmd = clap::Command::new("gws")
            .subcommand(clap::Command::new("files").subcommand(clap::Command::new("list")));

        let matches = cmd.get_matches_from(vec!["gws", "files", "list"]);
        let (method, _) = resolve_method_from_matches(&doc, &matches).unwrap();
        assert_eq!(method.id.as_deref(), Some("drive.files.list"));
    }

    #[test]
    fn test_resolve_method_from_matches_nested() {
        let mut resources = std::collections::HashMap::new();
        let mut files_res = crate::discovery::RestResource::default();
        let mut permissions_res = crate::discovery::RestResource::default();
        permissions_res.methods.insert(
            "get".to_string(),
            crate::discovery::RestMethod {
                id: Some("drive.files.permissions.get".to_string()),
                ..Default::default()
            },
        );
        files_res
            .resources
            .insert("permissions".to_string(), permissions_res);
        resources.insert("files".to_string(), files_res);

        let doc = discovery::RestDescription {
            name: "drive".to_string(),
            resources,
            ..Default::default()
        };

        let cmd =
            clap::Command::new("gws").subcommand(clap::Command::new("files").subcommand(
                clap::Command::new("permissions").subcommand(clap::Command::new("get")),
            ));

        let matches = cmd.get_matches_from(vec!["gws", "files", "permissions", "get"]);
        let (method, _) = resolve_method_from_matches(&doc, &matches).unwrap();
        assert_eq!(method.id.as_deref(), Some("drive.files.permissions.get"));
    }

    #[test]
    fn test_filter_args_strips_api_version() {
        let args: Vec<String> = vec![
            "gws".into(),
            "drive".into(),
            "--api-version".into(),
            "v3".into(),
            "files".into(),
            "list".into(),
        ];
        let filtered = filter_args_for_subcommand(&args, "drive");
        assert_eq!(filtered, vec!["gws", "files", "list"]);
    }

    #[test]
    fn test_filter_args_no_special_flags() {
        let args: Vec<String> = vec![
            "gws".into(),
            "drive".into(),
            "files".into(),
            "list".into(),
            "--format".into(),
            "table".into(),
        ];
        let filtered = filter_args_for_subcommand(&args, "drive");
        assert_eq!(filtered, vec!["gws", "files", "list", "--format", "table"]);
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
}

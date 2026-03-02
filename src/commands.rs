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
/// Returns None if the resource has no methods or sub-resources.
fn build_resource_command(name: &str, resource: &RestResource) -> Option<Command> {
    let mut cmd = Command::new(name.to_string())
        .about(format!("Operations on the '{name}' resource"))
        .subcommand_required(true)
        .arg_required_else_help(true);

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
}

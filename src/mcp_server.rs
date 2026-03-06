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

//! Model Context Protocol (MCP) server implementation.
//! Provides a stdio JSON-RPC server exposing Google Workspace APIs as MCP tools.

use crate::discovery::RestResource;
use crate::error::GwsError;
use crate::services;
use clap::{Arg, Command};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, Copy, PartialEq)]
enum ToolMode {
    Full,
    Compact,
}

#[derive(Debug, Clone)]
struct ServerConfig {
    services: Vec<String>,
    workflows: bool,
    _helpers: bool,
    tool_mode: ToolMode,
}

fn build_mcp_cli() -> Command {
    Command::new("mcp")
        .about("Starts the MCP server over stdio")
        .arg(
            Arg::new("services")
                .long("services")
                .short('s')
                .help("Comma separated list of services to expose (e.g., drive,gmail,all)")
                .default_value(""),
        )
        .arg(
            Arg::new("workflows")
                .long("workflows")
                .short('w')
                .action(clap::ArgAction::SetTrue)
                .help("Expose workflows as tools"),
        )
        .arg(
            Arg::new("helpers")
                .long("helpers")
                .short('e')
                .action(clap::ArgAction::SetTrue)
                .help("Expose service-specific helpers as tools"),
        )
        .arg(
            Arg::new("tool-mode")
                .long("tool-mode")
                .value_parser(["compact", "full"])
                .default_value("full")
                .help("Tool granularity: 'compact' (1 tool/service + discover) or 'full' (1 tool/method)"),
        )
}

pub async fn start(args: &[String]) -> Result<(), GwsError> {
    // Parse args
    let matches = build_mcp_cli().get_matches_from(args);
    let tool_mode = match matches.get_one::<String>("tool-mode").map(|s| s.as_str()) {
        Some("compact") => ToolMode::Compact,
        _ => ToolMode::Full,
    };
    let mut config = ServerConfig {
        services: Vec::new(),
        workflows: matches.get_flag("workflows"),
        _helpers: matches.get_flag("helpers"),
        tool_mode,
    };

    let svc_str = matches.get_one::<String>("services").unwrap();
    if !svc_str.is_empty() {
        if svc_str == "all" {
            config.services = services::SERVICES
                .iter()
                .map(|s| s.aliases[0].to_string())
                .collect();
        } else {
            config.services = svc_str.split(',').map(|s| s.trim().to_string()).collect();
        }
    }

    if config.services.is_empty() {
        eprintln!("[gws mcp] Warning: No services configured. Zero tools will be exposed.");
        eprintln!("[gws mcp] Re-run with: gws mcp -s <service> (e.g., -s drive,gmail,calendar)");
        eprintln!("[gws mcp] Use -s all to expose all available services.");
    } else {
        eprintln!(
            "[gws mcp] Starting with services: {}",
            config.services.join(", ")
        );
        eprintln!("[gws mcp] Tool mode: {:?}", config.tool_mode);
    }

    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    // Cache to hold generated tools configuration so we do not spam fetch from Google discovery
    let mut tools_cache = None;

    while let Ok(Some(line)) = stdin.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(&line) {
            Ok(req) => {
                let is_notification = req.get("id").is_none();
                let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
                let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

                let result = handle_request(method, &params, &config, &mut tools_cache).await;

                if !is_notification {
                    let id = req.get("id").unwrap();
                    let response = match result {
                        Ok(res) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": res
                        }),
                        Err(e) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32603,
                                "message": e.to_string()
                            }
                        }),
                    };

                    let mut out = match serde_json::to_string(&response) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("[gws mcp] Failed to serialize response: {e}");
                            continue;
                        }
                    };
                    out.push('\n');
                    let _ = stdout.write_all(out.as_bytes()).await;
                    let _ = stdout.flush().await;
                }
            }
            Err(_) => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {
                        "code": -32700,
                        "message": "Parse error"
                    }
                });
                let mut out = match serde_json::to_string(&response) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[gws mcp] Failed to serialize error response: {e}");
                        continue;
                    }
                };
                out.push('\n');
                let _ = stdout.write_all(out.as_bytes()).await;
                let _ = stdout.flush().await;
            }
        }
    }

    Ok(())
}

async fn handle_request(
    method: &str,
    params: &Value,
    config: &ServerConfig,
    tools_cache: &mut Option<Vec<Value>>,
) -> Result<Value, GwsError> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": {
                "name": "gws-mcp",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "tools": {}
            }
        })),
        "notifications/initialized" => {
            // Do nothing
            Ok(json!({}))
        }
        "tools/list" => {
            if tools_cache.is_none() {
                *tools_cache = Some(build_tools_list(config).await?);
            }
            Ok(json!({
                "tools": tools_cache.as_ref().unwrap()
            }))
        }
        "tools/call" => {
            // MCP spec: tool execution errors should be returned as successful results
            // with isError: true, NOT as JSON-RPC protocol errors. Returning JSON-RPC
            // errors causes clients to show generic "Tool execution failed" with no detail.
            match handle_tools_call(params, config).await {
                Ok(val) => Ok(val),
                Err(e) => Ok(json!({
                    "content": [{ "type": "text", "text": e.to_string() }],
                    "isError": true
                })),
            }
        }
        _ => Err(GwsError::Validation(format!(
            "Method not supported: {}",
            method
        ))),
    }
}

async fn build_tools_list(config: &ServerConfig) -> Result<Vec<Value>, GwsError> {
    if config.tool_mode == ToolMode::Compact {
        return build_compact_tools_list(config).await;
    }

    let mut tools = Vec::new();

    // 1. Walk core services
    for svc_name in &config.services {
        let (api_name, version) =
            crate::parse_service_and_version(&[svc_name.to_string()], svc_name)?;
        if let Ok(doc) = crate::discovery::fetch_discovery_document(&api_name, &version).await {
            walk_resources(&doc.name, &doc.resources, &mut tools);
        } else {
            eprintln!("[gws mcp] Warning: Failed to load discovery document for service '{}'. It will not be available as a tool.", svc_name);
        }
    }

    // 2. Workflows
    if config.workflows {
        append_workflow_tools(&mut tools);
    }

    Ok(tools)
}

async fn build_compact_tools_list(config: &ServerConfig) -> Result<Vec<Value>, GwsError> {
    let mut tools = Vec::new();

    for svc_name in &config.services {
        let (api_name, version) =
            crate::parse_service_and_version(&[svc_name.to_string()], svc_name)?;

        // Build description with resource names
        let description = if let Ok(doc) =
            crate::discovery::fetch_discovery_document(&api_name, &version).await
        {
            let mut resource_names = Vec::new();
            collect_resource_paths(&doc.resources, "", &mut resource_names);
            resource_names.sort();
            let svc_entry = services::SERVICES
                .iter()
                .find(|e| e.aliases.contains(&svc_name.as_str()));
            let desc = svc_entry.map(|e| e.description).unwrap_or("Google API");
            if resource_names.is_empty() {
                desc.to_string()
            } else {
                let names_str: Vec<&str> = resource_names.iter().map(|s| s.as_str()).collect();
                format!("{}. Resources: {}", desc, names_str.join(", "))
            }
        } else {
            eprintln!(
                "[gws mcp] Warning: Failed to load discovery document for '{}'. Tool will have minimal description.",
                svc_name
            );
            format!("Google Workspace API: {}", svc_name)
        };

        tools.push(json!({
            "name": svc_name,
            "description": description,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "resource": {
                        "type": "string",
                        "description": "Resource name (e.g., files, permissions)"
                    },
                    "method": {
                        "type": "string",
                        "description": "Method name (e.g., list, get, create)"
                    },
                    "params": {
                        "type": "object",
                        "description": "Query or path parameters"
                    },
                    "body": {
                        "type": "object",
                        "description": "Request body"
                    },
                    "upload": {
                        "type": "string",
                        "description": "Local file path to upload"
                    },
                    "page_all": {
                        "type": "boolean",
                        "description": "Auto-paginate, returning all pages"
                    }
                },
                "required": ["resource", "method"]
            }
        }));
    }

    // Add gws_discover meta-tool
    tools.push(json!({
        "name": "gws_discover",
        "description": "Query available resources, methods, and parameter schemas for any enabled service. Call with service only to list resources; add resource to list methods; add method to get full parameter schema.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "service": {
                    "type": "string",
                    "description": "Service name (e.g., drive, gmail)"
                },
                "resource": {
                    "type": "string",
                    "description": "Resource name to list methods for"
                },
                "method": {
                    "type": "string",
                    "description": "Method name to get full parameter schema"
                }
            },
            "required": ["service"]
        }
    }));

    // Workflows (same as full mode)
    if config.workflows {
        append_workflow_tools(&mut tools);
    }

    Ok(tools)
}

fn append_workflow_tools(tools: &mut Vec<Value>) {
    tools.push(json!({
        "name": "workflow_standup_report",
        "description": "Today's meetings + open tasks as a standup summary",
        "inputSchema": {
            "type": "object",
            "properties": {
                "format": { "type": "string", "description": "Output format: json, table, yaml, csv" }
            }
        }
    }));
    tools.push(json!({
        "name": "workflow_meeting_prep",
        "description": "Prepare for your next meeting: agenda, attendees, and linked docs",
        "inputSchema": {
            "type": "object",
            "properties": {
                "calendar": { "type": "string", "description": "Calendar ID (default: primary)" }
            }
        }
    }));
    tools.push(json!({
        "name": "workflow_email_to_task",
        "description": "Convert a Gmail message into a Google Tasks entry",
        "inputSchema": {
            "type": "object",
            "properties": {
                "message_id": { "type": "string", "description": "Gmail message ID" },
                "tasklist": { "type": "string", "description": "Task list ID" }
            },
            "required": ["message_id"]
        }
    }));
    tools.push(json!({
        "name": "workflow_weekly_digest",
        "description": "Weekly summary: this week's meetings + unread email count",
        "inputSchema": {
            "type": "object",
            "properties": {
                "format": { "type": "string", "description": "Output format" }
            }
        }
    }));
    tools.push(json!({
        "name": "workflow_file_announce",
        "description": "Announce a Drive file in a Chat space",
        "inputSchema": {
            "type": "object",
            "properties": {
                "file_id": { "type": "string", "description": "Drive file ID" },
                "space": { "type": "string", "description": "Chat space name" },
                "message": { "type": "string", "description": "Custom message" }
            },
            "required": ["file_id", "space"]
        }
    }));
}

fn walk_resources(prefix: &str, resources: &HashMap<String, RestResource>, tools: &mut Vec<Value>) {
    for (res_name, res) in resources {
        let new_prefix = format!("{}_{}", prefix, res_name);

        for (method_name, method) in &res.methods {
            let tool_name = format!("{}_{}", new_prefix, method_name);
            let mut description = method.description.clone().unwrap_or_default();
            if description.is_empty() {
                description = format!("Execute the {} Google API method", tool_name);
            }

            // Generate JSON Schema for MCP input — only include body/upload
            // when the Discovery Document method actually supports them.
            let mut properties = serde_json::Map::new();
            properties.insert(
                "params".to_string(),
                json!({
                    "type": "object",
                    "description": "Query or path parameters (e.g. fileId, q, pageSize)"
                }),
            );
            if method.request.is_some() {
                properties.insert(
                    "body".to_string(),
                    json!({
                        "type": "object",
                        "description": "Request body API object"
                    }),
                );
            }
            if method.supports_media_upload {
                properties.insert(
                    "upload".to_string(),
                    json!({
                        "type": "string",
                        "description": "Local file path to upload as media content"
                    }),
                );
            }
            if method.parameters.contains_key("pageToken") {
                properties.insert(
                    "page_all".to_string(),
                    json!({
                        "type": "boolean",
                        "description": "Auto-paginate, returning all pages"
                    }),
                );
            }
            let input_schema = json!({
                "type": "object",
                "properties": properties
            });

            tools.push(json!({
                "name": tool_name,
                "description": description,
                "inputSchema": input_schema
            }));
        }

        // Recurse into sub-resources
        if !res.resources.is_empty() {
            walk_resources(&new_prefix, &res.resources, tools);
        }
    }
}

async fn handle_discover(arguments: &Value, config: &ServerConfig) -> Result<Value, GwsError> {
    let service = arguments
        .get("service")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GwsError::Validation("Missing 'service' in gws_discover".to_string()))?;

    if !config.services.contains(&service.to_string()) {
        return Err(GwsError::Validation(format!(
            "Service '{}' is not enabled. Enabled: {}",
            service,
            config.services.join(", ")
        )));
    }

    let (api_name, version) = crate::parse_service_and_version(&[service.to_string()], service)?;
    let doc = crate::discovery::fetch_discovery_document(&api_name, &version).await?;

    let resource_name = arguments.get("resource").and_then(|v| v.as_str());
    let method_name = arguments.get("method").and_then(|v| v.as_str());

    let result = match (resource_name, method_name) {
        // Level 1: list all resources (recursively, with dot-separated paths)
        (None, _) => {
            let mut resource_entries = Vec::new();
            collect_resource_entries(&doc.resources, "", &mut resource_entries);
            json!({ "service": service, "resources": resource_entries })
        }
        // Level 2: list methods and sub-resources for a resource
        (Some(res), None) => {
            let mut all_paths = Vec::new();
            collect_resource_paths(&doc.resources, "", &mut all_paths);
            let resource = find_resource(&doc.resources, res).ok_or_else(|| {
                GwsError::Validation(format!(
                    "Resource '{}' not found in {}. Available: {}",
                    res,
                    service,
                    all_paths.join(", ")
                ))
            })?;
            let methods: Vec<Value> = resource
                .methods
                .iter()
                .map(|(name, m)| {
                    json!({
                        "name": name,
                        "httpMethod": m.http_method,
                        "description": m.description.as_deref().unwrap_or("")
                    })
                })
                .collect();
            let sub_resources: Vec<&str> = resource.resources.keys().map(|s| s.as_str()).collect();
            let mut result = json!({ "service": service, "resource": res, "methods": methods });
            if !sub_resources.is_empty() {
                result["subResources"] = json!(sub_resources);
            }
            result
        }
        // Level 3: full param schema for a method
        (Some(res), Some(meth)) => {
            let resource = find_resource(&doc.resources, res).ok_or_else(|| {
                let mut all_paths = Vec::new();
                collect_resource_paths(&doc.resources, "", &mut all_paths);
                GwsError::Validation(format!(
                    "Resource '{}' not found in {}. Available: {}",
                    res,
                    service,
                    all_paths.join(", ")
                ))
            })?;
            let method = resource.methods.get(meth).ok_or_else(|| {
                GwsError::Validation(format!(
                    "Method '{}' not found in {}.{}. Available: {}",
                    meth,
                    service,
                    res,
                    resource
                        .methods
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            let params: Vec<Value> = method
                .parameters
                .iter()
                .map(|(name, p)| {
                    json!({
                        "name": name,
                        "type": p.param_type.as_deref().unwrap_or("string"),
                        "required": p.required,
                        "location": p.location.as_deref().unwrap_or("query"),
                        "description": p.description.as_deref().unwrap_or("")
                    })
                })
                .collect();
            json!({
                "service": service,
                "resource": res,
                "method": meth,
                "httpMethod": method.http_method,
                "description": method.description.as_deref().unwrap_or(""),
                "parameters": params,
                "supportsMediaUpload": method.supports_media_upload,
                "supportsMediaDownload": method.supports_media_download
            })
        }
    };

    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default() }],
        "isError": false
    }))
}

/// Recursively collect all resource paths (dot-separated) from a resource tree.
fn collect_resource_paths(
    resources: &HashMap<String, RestResource>,
    prefix: &str,
    out: &mut Vec<String>,
) {
    for (name, res) in resources {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        out.push(path.clone());
        if !res.resources.is_empty() {
            collect_resource_paths(&res.resources, &path, out);
        }
    }
}

/// Recursively collect resource entries (name + methods) for discover Level 1.
fn collect_resource_entries(
    resources: &HashMap<String, RestResource>,
    prefix: &str,
    out: &mut Vec<Value>,
) {
    for (name, res) in resources {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        let methods: Vec<&str> = res.methods.keys().map(|s| s.as_str()).collect();
        if !methods.is_empty() {
            out.push(json!({
                "name": path.clone(),
                "methods": methods
            }));
        }
        if !res.resources.is_empty() {
            collect_resource_entries(&res.resources, &path, out);
        }
    }
}

/// Walk into potentially nested resources by dot-separated path (e.g., "projects.locations.templates").
fn find_resource<'a>(
    resources: &'a HashMap<String, RestResource>,
    path: &str,
) -> Option<&'a RestResource> {
    let mut segments = path.split('.');
    let first_segment = segments.next()?;
    let mut current_res = resources.get(first_segment)?;
    for segment in segments {
        current_res = current_res.resources.get(segment)?;
    }
    Some(current_res)
}

async fn handle_tools_call(params: &Value, config: &ServerConfig) -> Result<Value, GwsError> {
    let tool_name = params
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| GwsError::Validation("Missing 'name' in tools/call".to_string()))?;

    let default_args = json!({});
    let arguments = params.get("arguments").unwrap_or(&default_args);

    if tool_name.starts_with("workflow_") {
        return Err(GwsError::Other(anyhow::anyhow!(
            "Workflows are not yet fully implemented via MCP"
        )));
    }

    if tool_name == "gws_discover" {
        return handle_discover(arguments, config).await;
    }

    // Compact mode: tool_name IS the service alias, resource/method are in arguments
    if config.tool_mode == ToolMode::Compact {
        let resource_path = arguments
            .get("resource")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GwsError::Validation("Missing 'resource' argument".to_string()))?;
        let method_name = arguments
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GwsError::Validation("Missing 'method' argument".to_string()))?;

        let svc_alias = tool_name;
        if !config.services.contains(&svc_alias.to_string()) {
            return Err(GwsError::Validation(format!(
                "Service '{}' is not enabled in this MCP session",
                svc_alias
            )));
        }

        let (api_name, version) =
            crate::parse_service_and_version(&[svc_alias.to_string()], svc_alias)?;
        let doc = crate::discovery::fetch_discovery_document(&api_name, &version).await?;

        let resource = find_resource(&doc.resources, resource_path).ok_or_else(|| {
            GwsError::Validation(format!(
                "Resource '{}' not found in {}",
                resource_path, svc_alias
            ))
        })?;

        let method = resource.methods.get(method_name).ok_or_else(|| {
            GwsError::Validation(format!(
                "Method '{}' not found in {}.{}",
                method_name, svc_alias, resource_path
            ))
        })?;

        return execute_mcp_method(&doc, method, arguments).await;
    }

    // Full mode: tool_name encodes service_resource_method (e.g., drive_files_list)
    let parts: Vec<&str> = tool_name.split('_').collect();
    if parts.len() < 3 {
        return Err(GwsError::Validation(format!(
            "Invalid API tool name: {}",
            tool_name
        )));
    }

    let svc_alias = parts[0];

    if !config.services.contains(&svc_alias.to_string()) {
        return Err(GwsError::Validation(format!(
            "Service '{}' is not enabled in this MCP session",
            svc_alias
        )));
    }

    let (api_name, version) =
        crate::parse_service_and_version(&[svc_alias.to_string()], svc_alias)?;
    let doc = crate::discovery::fetch_discovery_document(&api_name, &version).await?;

    let mut current_resources = &doc.resources;
    let mut current_res = None;

    // Walk: ["drive", "files", "list"] — iterate resource path segments between service and method
    for res_name in &parts[1..parts.len() - 1] {
        if let Some(res) = current_resources.get(*res_name) {
            current_res = Some(res);
            current_resources = &res.resources;
        } else {
            return Err(GwsError::Validation(format!(
                "Resource '{}' not found in Discovery Document",
                res_name
            )));
        }
    }

    let method_name = parts.last().unwrap();
    let method = if let Some(res) = current_res {
        res.methods
            .get(*method_name)
            .ok_or_else(|| GwsError::Validation(format!("Method '{}' not found", method_name)))?
    } else {
        return Err(GwsError::Validation("Resource not found".to_string()));
    };

    execute_mcp_method(&doc, method, arguments).await
}

async fn execute_mcp_method(
    doc: &crate::discovery::RestDescription,
    method: &crate::discovery::RestMethod,
    arguments: &Value,
) -> Result<Value, GwsError> {
    let params_json_val = arguments.get("params");
    let params_str = params_json_val
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| GwsError::Validation(format!("Failed to serialize params: {e}")))?;

    // Drop empty body objects — LLMs commonly send "body": {} even on GET
    // methods, which causes Google APIs to return 400.
    let body_json_val = arguments
        .get("body")
        .filter(|v| !v.as_object().is_some_and(|m| m.is_empty()));
    let body_str = body_json_val
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| GwsError::Validation(format!("Failed to serialize body: {e}")))?;

    let upload_path = if let Some(raw) = arguments
        .get("upload")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        let p = std::path::Path::new(raw);
        if p.is_absolute() || p.components().any(|c| c == std::path::Component::ParentDir) {
            return Err(GwsError::Validation(format!(
                "Upload path '{}' is not allowed. Paths must be relative and within the current directory.",
                raw
            )));
        }
        Some(raw)
    } else {
        None
    };

    let page_all = arguments
        .get("page_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let pagination = crate::executor::PaginationConfig {
        page_all,
        page_limit: 100,
        page_delay_ms: 100,
    };

    let scopes: Vec<&str> = crate::select_scope(&method.scopes).into_iter().collect();
    let account = std::env::var("GOOGLE_WORKSPACE_CLI_ACCOUNT").ok();
    let (token, auth_method) = match crate::executor::resolve_auth(&scopes, account.as_deref()).await {
        Ok(auth) => auth,
        Err(e) => {
            eprintln!(
                "[gws mcp] Warning: Authentication failed, proceeding without credentials: {e}"
            );
            (None, crate::executor::AuthMethod::None)
        }
    };

    let result = crate::executor::execute_method(
        doc,
        method,
        params_str.as_deref(),
        body_str.as_deref(),
        token.as_deref(),
        auth_method,
        None,
        upload_path,
        false,
        &pagination,
        None,
        &crate::helpers::modelarmor::SanitizeMode::Warn,
        &crate::formatter::OutputFormat::default(),
        true,
    )
    .await?;

    let text_content = match result {
        Some(val) => serde_json::to_string_pretty(&val).unwrap_or_else(|_| "[]".to_string()),
        None => "Execution completed with no output.".to_string(),
    };

    Ok(json!({
        "content": [
            {
                "type": "text",
                "text": text_content
            }
        ],
        "isError": false
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{MethodParameter, RestDescription, RestMethod, RestResource};
    use std::collections::HashMap;

    fn mock_config_compact(services: Vec<&str>) -> ServerConfig {
        ServerConfig {
            services: services.into_iter().map(String::from).collect(),
            workflows: false,
            _helpers: false,
            tool_mode: ToolMode::Compact,
        }
    }

    fn mock_doc() -> RestDescription {
        let mut params = HashMap::new();
        params.insert(
            "fileId".to_string(),
            MethodParameter {
                param_type: Some("string".to_string()),
                required: true,
                location: Some("path".to_string()),
                description: Some("The ID of the file".to_string()),
                ..Default::default()
            },
        );
        params.insert(
            "fields".to_string(),
            MethodParameter {
                param_type: Some("string".to_string()),
                required: false,
                location: Some("query".to_string()),
                description: Some("Selector specifying fields".to_string()),
                ..Default::default()
            },
        );

        let mut methods = HashMap::new();
        methods.insert(
            "list".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                path: "files".to_string(),
                description: Some("Lists files".to_string()),
                ..Default::default()
            },
        );
        methods.insert(
            "get".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                path: "files/{fileId}".to_string(),
                description: Some("Gets a file".to_string()),
                parameters: params,
                ..Default::default()
            },
        );

        let mut resources = HashMap::new();
        resources.insert(
            "files".to_string(),
            RestResource {
                methods,
                ..Default::default()
            },
        );

        RestDescription {
            name: "drive".to_string(),
            resources,
            ..Default::default()
        }
    }

    /// Mock a nested doc like Gmail: users -> messages, threads
    fn mock_nested_doc() -> RestDescription {
        let mut msg_methods = HashMap::new();
        msg_methods.insert(
            "list".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                path: "messages".to_string(),
                description: Some("Lists messages".to_string()),
                ..Default::default()
            },
        );
        msg_methods.insert(
            "get".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                path: "messages/{id}".to_string(),
                description: Some("Gets a message".to_string()),
                ..Default::default()
            },
        );
        let messages = RestResource {
            methods: msg_methods,
            ..Default::default()
        };

        let mut thread_methods = HashMap::new();
        thread_methods.insert(
            "list".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                path: "threads".to_string(),
                ..Default::default()
            },
        );
        let threads = RestResource {
            methods: thread_methods,
            ..Default::default()
        };

        let mut user_methods = HashMap::new();
        user_methods.insert(
            "getProfile".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                path: "users/{userId}/profile".to_string(),
                ..Default::default()
            },
        );

        let mut sub_resources = HashMap::new();
        sub_resources.insert("messages".to_string(), messages);
        sub_resources.insert("threads".to_string(), threads);

        let users = RestResource {
            methods: user_methods,
            resources: sub_resources,
        };

        let mut resources = HashMap::new();
        resources.insert("users".to_string(), users);

        RestDescription {
            name: "gmail".to_string(),
            resources,
            ..Default::default()
        }
    }

    // -- find_resource tests --

    #[test]
    fn test_find_resource_top_level() {
        let doc = mock_doc();
        let res = find_resource(&doc.resources, "files");
        assert!(res.is_some());
        assert!(res.unwrap().methods.contains_key("list"));
    }

    #[test]
    fn test_find_resource_not_found() {
        let doc = mock_doc();
        assert!(find_resource(&doc.resources, "missing").is_none());
    }

    #[test]
    fn test_find_resource_nested_dot_path() {
        let mut inner_methods = HashMap::new();
        inner_methods.insert(
            "create".to_string(),
            RestMethod {
                http_method: "POST".to_string(),
                path: "permissions".to_string(),
                ..Default::default()
            },
        );
        let inner = RestResource {
            methods: inner_methods,
            ..Default::default()
        };
        let mut sub_resources = HashMap::new();
        sub_resources.insert("permissions".to_string(), inner);

        let outer = RestResource {
            resources: sub_resources,
            ..Default::default()
        };
        let mut top = HashMap::new();
        top.insert("files".to_string(), outer);

        let res = find_resource(&top, "files.permissions");
        assert!(res.is_some());
        assert!(res.unwrap().methods.contains_key("create"));
    }

    // -- collect_resource_paths tests --

    #[test]
    fn test_collect_resource_paths_flat() {
        let doc = mock_doc();
        let mut paths = Vec::new();
        collect_resource_paths(&doc.resources, "", &mut paths);
        paths.sort();
        assert_eq!(paths, vec!["files"]);
    }

    #[test]
    fn test_collect_resource_paths_nested() {
        let doc = mock_nested_doc();
        let mut paths = Vec::new();
        collect_resource_paths(&doc.resources, "", &mut paths);
        paths.sort();
        assert!(paths.contains(&"users".to_string()));
        assert!(paths.contains(&"users.messages".to_string()));
    }

    // -- collect_resource_entries tests --

    #[test]
    fn test_collect_resource_entries_includes_nested() {
        let doc = mock_nested_doc();
        let mut entries = Vec::new();
        collect_resource_entries(&doc.resources, "", &mut entries);
        let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
        assert!(names.contains(&"users"));
        assert!(names.contains(&"users.messages"));
    }

    // -- handle_discover tests --

    #[tokio::test]
    async fn test_discover_service_not_enabled() {
        let config = mock_config_compact(vec!["gmail"]);
        let args = json!({"service": "drive"});

        let result = handle_discover(&args, &config).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not enabled"));
    }

    #[tokio::test]
    async fn test_discover_missing_service_arg() {
        let config = mock_config_compact(vec!["drive"]);
        let args = json!({});

        let result = handle_discover(&args, &config).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Missing 'service'"));
    }

    // -- ToolMode tests --

    #[test]
    fn test_tool_mode_enum_equality() {
        assert_eq!(ToolMode::Compact, ToolMode::Compact);
        assert_ne!(ToolMode::Compact, ToolMode::Full);
    }

    // -- CLI parsing tests --

    #[test]
    fn test_cli_tool_mode_default_is_full() {
        let cli = build_mcp_cli();
        let matches = cli.get_matches_from(vec!["mcp"]);
        let mode = matches.get_one::<String>("tool-mode").unwrap();
        assert_eq!(mode, "full");
    }

    #[test]
    fn test_cli_tool_mode_compact() {
        let cli = build_mcp_cli();
        let matches = cli.get_matches_from(vec!["mcp", "--tool-mode", "compact"]);
        let mode = matches.get_one::<String>("tool-mode").unwrap();
        assert_eq!(mode, "compact");
    }

    #[test]
    fn test_cli_tool_mode_invalid_rejected() {
        let cli = build_mcp_cli();
        let result = cli.try_get_matches_from(vec!["mcp", "--tool-mode", "invalid"]);
        assert!(result.is_err());
    }

    // -- append_workflow_tools tests --

    #[test]
    fn test_append_workflow_tools_adds_five() {
        let mut tools = Vec::new();
        append_workflow_tools(&mut tools);
        assert_eq!(tools.len(), 5);
        assert_eq!(tools[0]["name"], "workflow_standup_report");
        assert_eq!(tools[4]["name"], "workflow_file_announce");
    }
}

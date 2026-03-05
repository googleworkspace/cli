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

#[derive(Debug, Clone)]
struct ServerConfig {
    services: Vec<String>,
    workflows: bool,
    _helpers: bool,
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
}

pub async fn start(args: &[String]) -> Result<(), GwsError> {
    // Parse args
    let matches = build_mcp_cli().get_matches_from(args);
    let mut config = ServerConfig {
        services: Vec::new(),
        workflows: matches.get_flag("workflows"),
        _helpers: matches.get_flag("helpers"),
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

                    let mut out = serde_json::to_string(&response).unwrap();
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
                let mut out = serde_json::to_string(&response).unwrap();
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
        "tools/call" => handle_tools_call(params, config).await,
        _ => Err(GwsError::Validation(format!(
            "Method not supported: {}",
            method
        ))),
    }
}

async fn build_tools_list(config: &ServerConfig) -> Result<Vec<Value>, GwsError> {
    let mut tools = Vec::new();

    // 1. Walk core services
    for svc_name in &config.services {
        let (api_name, version) =
            crate::parse_service_and_version(std::slice::from_ref(svc_name), svc_name)?;
        if let Ok(doc) = crate::discovery::fetch_discovery_document(&api_name, &version).await {
            walk_resources(&doc.name, &doc.resources, &mut tools);
        } else {
            eprintln!("[gws mcp] Warning: Failed to load discovery document for service '{}'. It will not be available as a tool.", svc_name);
        }
    }

    // 2. Helpers and Workflows (Not fully mapped yet, but structure is here)
    if config.workflows {
        // Expose workflows
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

    Ok(tools)
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

            // Generate JSON Schema for MCP input
            let input_schema = json!({
                "type": "object",
                "properties": {
                    "params": {
                        "type": "object",
                        "description": "Query or path parameters (e.g. fileId, q, pageSize)"
                    },
                    "body": {
                        "type": "object",
                        "description": "Request body API object"
                    },
                    "upload": {
                        "type": "string",
                        "description": "Local file path to upload as media content"
                    },
                    "page_all": {
                        "type": "boolean",
                        "description": "Auto-paginate, returning all pages"
                    }
                }
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

    let params_json_val = arguments.get("params");
    let params_str = params_json_val.map(|v| serde_json::to_string(v).unwrap());

    let body_json_val = arguments.get("body");
    let body_str = body_json_val.map(|v| serde_json::to_string(v).unwrap());

    // Security: validate upload path to prevent arbitrary local file reads.
    // Only allow paths within the current working directory.
    let upload_path = if let Some(raw) = arguments.get("upload").and_then(|v| v.as_str()) {
        let p = std::path::Path::new(raw);
        // Reject absolute paths and any path that escapes cwd via "../"
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
        page_limit: 100, // Safe default for MCP
        page_delay_ms: 100,
    };

    let scopes: Vec<&str> = method.scopes.iter().map(|s| s.as_str()).collect();
    let (token, auth_method) = crate::executor::resolve_auth(&scopes).await;

    let result = crate::executor::execute_method(
        &doc,
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
        true, // capture_output = true!
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

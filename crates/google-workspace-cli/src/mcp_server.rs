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
pub(crate) enum ToolMode {
    Full,
    Compact,
}

#[derive(Debug, Clone)]
pub(crate) struct ServerConfig {
    services: Vec<String>,
    workflows: bool,
    helpers: bool,
    tool_mode: ToolMode,
}

fn build_mcp_cli() -> Command {
    Command::new("mcp")
        .about("Starts the MCP server (stdio by default, or HTTP with --transport http)")
        .arg(
            Arg::new("transport")
                .long("transport")
                .value_parser(["stdio", "http"])
                .default_value("stdio")
                .help("Transport mode: 'stdio' (default) or 'http' (Streamable HTTP)"),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .short('p')
                .value_parser(clap::value_parser!(u16))
                .default_value("3000")
                .help("Port to listen on (HTTP transport only)"),
        )
        .arg(
            Arg::new("bind")
                .long("bind")
                .default_value("127.0.0.1")
                .help("Address to bind (HTTP transport only). Use 0.0.0.0 to allow external access"),
        )
        .arg(
            Arg::new("auth")
                .long("auth")
                .action(clap::ArgAction::SetTrue)
                .help("Enable OAuth2 PKCE authentication (HTTP transport only). Requires client_secret.json from `gws auth setup`"),
        )
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
    let matches = build_mcp_cli().get_matches_from(args);
    let tool_mode = match matches.get_one::<String>("tool-mode").map(|s| s.as_str()) {
        Some("compact") => ToolMode::Compact,
        _ => ToolMode::Full,
    };
    let mut config = ServerConfig {
        services: Vec::new(),
        workflows: matches.get_flag("workflows"),
        helpers: matches.get_flag("helpers"),
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

    let transport = matches
        .get_one::<String>("transport")
        .map(|s| s.as_str())
        .unwrap_or("stdio");

    if transport == "http" {
        let port = *matches.get_one::<u16>("port").unwrap_or(&3000);
        let bind = matches
            .get_one::<String>("bind")
            .map(|s| s.as_str())
            .unwrap_or("127.0.0.1")
            .to_string();
        let enable_auth = matches.get_flag("auth");
        return crate::mcp_http_server::start_http(config, port, bind, enable_auth).await;
    }

    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

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
        "notifications/initialized" => Ok(json!({})),
        "tools/list" => {
            if tools_cache.is_none() {
                *tools_cache = Some(build_tools_list(config).await?);
            }
            Ok(json!({
                "tools": tools_cache.as_ref().unwrap()
            }))
        }
        "tools/call" => match handle_tools_call(params, config).await {
            Ok(val) => Ok(val),
            Err(e) => Ok(json!({
                "content": [{ "type": "text", "text": e.to_string() }],
                "isError": true
            })),
        },
        _ => Err(GwsError::Validation(format!(
            "Method not supported: {}",
            method
        ))),
    }
}

pub(crate) async fn build_tools_list(config: &ServerConfig) -> Result<Vec<Value>, GwsError> {
    if config.tool_mode == ToolMode::Compact {
        return build_compact_tools_list(config).await;
    }

    let mut tools = Vec::new();

    for svc_name in &config.services {
        let (api_name, version) =
            crate::parse_service_and_version(&[svc_name.to_string()], svc_name)?;
        if let Ok(doc) = crate::discovery::fetch_discovery_document(&api_name, &version).await {
            walk_resources(svc_name, &doc.resources, &mut tools);
        } else {
            eprintln!("[gws mcp] Warning: Failed to load discovery document for service '{}'. It will not be available as a tool.", svc_name);
        }
    }

    if config.helpers {
        append_helper_tools(&config.services, &mut tools);
    }

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

    if config.helpers {
        append_helper_tools(&config.services, &mut tools);
    }

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

fn append_helper_tools(services: &[String], tools: &mut Vec<Value>) {
    if services.iter().any(|s| s == "gmail") {
        tools.push(json!({
            "name": "gmail_send",
            "description": "Send an email with plain text body. Handles RFC 2822 formatting and base64 encoding automatically — no need to construct raw messages manually.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Recipient email address(es), comma-separated"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Email subject"
                    },
                    "body": {
                        "type": "string",
                        "description": "Email body (plain text)"
                    },
                    "cc": {
                        "type": "string",
                        "description": "CC email address(es), comma-separated"
                    },
                    "bcc": {
                        "type": "string",
                        "description": "BCC email address(es), comma-separated"
                    }
                },
                "required": ["to", "subject", "body"]
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
            }
        }));
        tools.push(json!({
            "name": "gmail_reply",
            "description": "Reply to an email within its existing thread. Automatically sets threading headers (In-Reply-To, References) and Re: subject prefix. Use gmail_users_messages_list or gmail_users_messages_get to find the message_id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": {
                        "type": "string",
                        "description": "Gmail message ID to reply to"
                    },
                    "body": {
                        "type": "string",
                        "description": "Reply body (plain text)"
                    },
                    "reply_all": {
                        "type": "boolean",
                        "description": "Reply to all recipients instead of just the sender (default: false)"
                    },
                    "cc": {
                        "type": "string",
                        "description": "Additional CC email address(es), comma-separated"
                    },
                    "bcc": {
                        "type": "string",
                        "description": "BCC email address(es), comma-separated"
                    }
                },
                "required": ["message_id", "body"]
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
            }
        }));
    }
}

// ---------------------------------------------------------------------------
// Common MCP helper utilities
// ---------------------------------------------------------------------------

/// Extract a required string parameter from MCP tool arguments.
fn get_required_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, GwsError> {
    args.get(name)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| GwsError::Validation(format!("Missing '{name}' parameter")))
}

/// Extract an optional string parameter from MCP tool arguments.
fn get_optional_str<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
}

/// Send a raw RFC 2822 message via Gmail API.
///
/// Shared by `handle_gmail_send` and `handle_gmail_reply`.
async fn send_raw_gmail(
    raw_message: &str,
    thread_id: Option<&str>,
    draft: bool,
) -> Result<Value, GwsError> {
    let (api_name, version) = crate::parse_service_and_version(&["gmail".to_string()], "gmail")?;
    let doc = crate::discovery::fetch_discovery_document(&api_name, &version).await?;
    let method = crate::helpers::gmail::resolve_mail_method(&doc, draft)?;
    let metadata = crate::helpers::gmail::mcp_build_send_metadata(thread_id, draft);

    let params = json!({ "userId": "me" });
    let params_str = params.to_string();

    let scopes: Vec<&str> = crate::select_scope(&method.scopes).into_iter().collect();
    let (token, auth_method) = match crate::auth::get_token(&scopes).await {
        Ok(t) => (Some(t), crate::executor::AuthMethod::OAuth),
        Err(e) => return Err(GwsError::Auth(format!("Gmail auth failed: {e}"))),
    };

    let pagination = crate::executor::PaginationConfig {
        page_all: false,
        page_limit: 10,
        page_delay_ms: 100,
    };

    let result = crate::executor::execute_method(
        &doc,
        method,
        Some(&params_str),
        metadata.as_deref(),
        token.as_deref(),
        auth_method,
        None,
        Some(crate::executor::UploadSource::Bytes {
            data: raw_message.as_bytes(),
            content_type: "message/rfc822",
        }),
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
        None => "Operation completed successfully.".to_string(),
    };

    Ok(json!({
        "content": [{ "type": "text", "text": text_content }],
        "isError": false
    }))
}

// ---------------------------------------------------------------------------
// Gmail helper handlers
// ---------------------------------------------------------------------------

async fn handle_gmail_send(arguments: &Value) -> Result<Value, GwsError> {
    let to = get_required_str(arguments, "to")?;
    let subject = get_required_str(arguments, "subject")?;
    let body_text = get_required_str(arguments, "body")?;
    let cc_str = get_optional_str(arguments, "cc");
    let bcc_str = get_optional_str(arguments, "bcc");

    let to_mailboxes = crate::helpers::gmail::Mailbox::parse_list(to);
    if to_mailboxes.is_empty() {
        return Err(GwsError::Validation(
            "'to' must specify at least one recipient".to_string(),
        ));
    }
    let cc_mailboxes = cc_str.map(crate::helpers::gmail::Mailbox::parse_list);
    let bcc_mailboxes = bcc_str.map(crate::helpers::gmail::Mailbox::parse_list);

    let mb = mail_builder::MessageBuilder::new()
        .to(crate::helpers::gmail::to_mb_address_list(&to_mailboxes))
        .subject(subject);

    let mb = crate::helpers::gmail::apply_optional_headers(
        mb,
        None,
        cc_mailboxes.as_deref(),
        bcc_mailboxes.as_deref(),
    );

    let raw_message = crate::helpers::gmail::finalize_message(mb, body_text, false, &[])?;

    send_raw_gmail(&raw_message, None, false).await
}

async fn handle_gmail_reply(arguments: &Value) -> Result<Value, GwsError> {
    let message_id = get_required_str(arguments, "message_id")?;
    let body_text = get_required_str(arguments, "body")?;
    let reply_all = arguments
        .get("reply_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let cc_str = get_optional_str(arguments, "cc");
    let bcc_str = get_optional_str(arguments, "bcc");

    let cc_mailboxes = cc_str.map(crate::helpers::gmail::Mailbox::parse_list);
    let bcc_mailboxes = bcc_str.map(crate::helpers::gmail::Mailbox::parse_list);

    let (raw_message, thread_id) = crate::helpers::gmail::mcp_compose_reply(
        message_id,
        body_text,
        reply_all,
        cc_mailboxes.as_deref(),
        bcc_mailboxes.as_deref(),
    )
    .await?;

    send_raw_gmail(&raw_message, thread_id.as_deref(), false).await
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
                "inputSchema": input_schema,
                "annotations": http_method_annotations(&method.http_method),
            }));
        }

        if !res.resources.is_empty() {
            walk_resources(&new_prefix, &res.resources, tools);
        }
    }
}

/// MCP tool annotations derived from HTTP method (upstream #260).
fn http_method_annotations(http_method: &str) -> Value {
    let m = http_method.to_ascii_uppercase();
    json!({
        "readOnlyHint": m == "GET",
        "destructiveHint": m == "DELETE",
        "idempotentHint": matches!(m.as_str(), "GET" | "PUT" | "DELETE" | "HEAD"),
    })
}

/// Greedy resolver for underscore-joined tool names (upstream #170).
///
/// Resource names may themselves contain underscores (e.g. `role_assignments`),
/// so `split('_')` is ambiguous. This walks the Discovery tree and consumes
/// `resource_name_` prefixes greedily, supporting arbitrarily nested resources.
fn resolve_tool_path(
    remaining: &str,
    resources: &HashMap<String, RestResource>,
) -> Option<(Vec<String>, String)> {
    for (res_name, res) in resources {
        let prefix = format!("{}_", res_name);
        if let Some(after) = remaining.strip_prefix(&prefix) {
            if res.methods.contains_key(after) {
                return Some((vec![res_name.clone()], after.to_string()));
            }
            if let Some((mut sub_path, method)) = resolve_tool_path(after, &res.resources) {
                sub_path.insert(0, res_name.clone());
                return Some((sub_path, method));
            }
        }
    }
    None
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
        (None, _) => {
            let mut resource_entries = Vec::new();
            collect_resource_entries(&doc.resources, "", &mut resource_entries);
            json!({ "service": service, "resources": resource_entries })
        }
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

pub(crate) async fn handle_tools_call(
    params: &Value,
    config: &ServerConfig,
) -> Result<Value, GwsError> {
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

    // Helper tool dispatch
    let helper_result = match tool_name {
        "gmail_send" => Some(handle_gmail_send(arguments).await),
        "gmail_reply" => Some(handle_gmail_reply(arguments).await),
        _ => None,
    };
    if let Some(result) = helper_result {
        if !config.helpers {
            return Err(GwsError::Validation(
                "Helper tools are not enabled. Re-run with --helpers flag.".to_string(),
            ));
        }
        return result;
    }

    // Compact mode
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

    // Full mode — greedy parse that handles resource names containing underscores
    // (upstream #170, e.g. `admin_role_assignments_list`).
    let (svc_alias, remaining) = tool_name
        .split_once('_')
        .ok_or_else(|| GwsError::Validation(format!("Invalid API tool name: {}", tool_name)))?;

    if !config.services.contains(&svc_alias.to_string()) {
        return Err(GwsError::Validation(format!(
            "Service '{}' is not enabled in this MCP session",
            svc_alias
        )));
    }

    let (api_name, version) =
        crate::parse_service_and_version(&[svc_alias.to_string()], svc_alias)?;
    let doc = crate::discovery::fetch_discovery_document(&api_name, &version).await?;

    let (resource_path, method_name) =
        resolve_tool_path(remaining, &doc.resources).ok_or_else(|| {
            GwsError::Validation(format!(
                "Tool '{}' not found in Discovery Document",
                tool_name
            ))
        })?;

    let mut current_resources = &doc.resources;
    let mut current_res = None;
    for res_name in &resource_path {
        let res = current_resources
            .get(res_name)
            .ok_or_else(|| GwsError::Validation(format!("Resource '{}' not found", res_name)))?;
        current_res = Some(res);
        current_resources = &res.resources;
    }
    let method = current_res
        .and_then(|r| r.methods.get(&method_name))
        .ok_or_else(|| GwsError::Validation(format!("Method '{}' not found", method_name)))?;

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

    let body_json_val = arguments
        .get("body")
        .filter(|v| !v.as_object().is_some_and(|m| m.is_empty()));
    let body_str = body_json_val
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| GwsError::Validation(format!("Failed to serialize body: {e}")))?;

    let upload_source = if let Some(raw) = arguments
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
        Some(crate::executor::UploadSource::File {
            path: raw,
            content_type: None,
        })
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
    let (token, auth_method) = match crate::auth::get_token(&scopes).await {
        Ok(t) => (Some(t), crate::executor::AuthMethod::OAuth),
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
        upload_source,
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

    #[test]
    fn test_http_method_annotations_get() {
        let a = http_method_annotations("GET");
        assert_eq!(a["readOnlyHint"], true);
        assert_eq!(a["destructiveHint"], false);
        assert_eq!(a["idempotentHint"], true);
    }

    #[test]
    fn test_http_method_annotations_delete() {
        let a = http_method_annotations("DELETE");
        assert_eq!(a["readOnlyHint"], false);
        assert_eq!(a["destructiveHint"], true);
        assert_eq!(a["idempotentHint"], true);
    }

    #[test]
    fn test_http_method_annotations_post() {
        let a = http_method_annotations("POST");
        assert_eq!(a["readOnlyHint"], false);
        assert_eq!(a["destructiveHint"], false);
        assert_eq!(a["idempotentHint"], false);
    }

    #[test]
    fn test_resolve_tool_path_simple() {
        let mut methods = HashMap::new();
        methods.insert(
            "list".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                ..Default::default()
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

        let (path, method) = resolve_tool_path("files_list", &resources).unwrap();
        assert_eq!(path, vec!["files".to_string()]);
        assert_eq!(method, "list");
    }

    #[test]
    fn test_resolve_tool_path_multi_word_resource() {
        // Regression for upstream #170: resource name contains underscore.
        let mut methods = HashMap::new();
        methods.insert(
            "list".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                ..Default::default()
            },
        );
        let mut resources = HashMap::new();
        resources.insert(
            "role_assignments".to_string(),
            RestResource {
                methods,
                resources: HashMap::new(),
            },
        );

        let (path, method) = resolve_tool_path("role_assignments_list", &resources).unwrap();
        assert_eq!(path, vec!["role_assignments".to_string()]);
        assert_eq!(method, "list");
    }

    #[test]
    fn test_resolve_tool_path_nested() {
        let mut inner_methods = HashMap::new();
        inner_methods.insert(
            "list".to_string(),
            RestMethod {
                http_method: "GET".to_string(),
                ..Default::default()
            },
        );
        let mut inner_resources = HashMap::new();
        inner_resources.insert(
            "space_events".to_string(),
            RestResource {
                methods: inner_methods,
                resources: HashMap::new(),
            },
        );
        let mut outer_resources = HashMap::new();
        outer_resources.insert(
            "spaces".to_string(),
            RestResource {
                methods: HashMap::new(),
                resources: inner_resources,
            },
        );

        let (path, method) =
            resolve_tool_path("spaces_space_events_list", &outer_resources).unwrap();
        assert_eq!(path, vec!["spaces".to_string(), "space_events".to_string()]);
        assert_eq!(method, "list");
    }

    #[test]
    fn test_resolve_tool_path_not_found() {
        let resources = HashMap::new();
        assert!(resolve_tool_path("nonexistent_method", &resources).is_none());
    }

    fn mock_config_compact(services: Vec<&str>) -> ServerConfig {
        ServerConfig {
            services: services.into_iter().map(String::from).collect(),
            workflows: false,
            helpers: false,
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

    #[test]
    fn test_collect_resource_entries_includes_nested() {
        let doc = mock_nested_doc();
        let mut entries = Vec::new();
        collect_resource_entries(&doc.resources, "", &mut entries);
        let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
        assert!(names.contains(&"users"));
        assert!(names.contains(&"users.messages"));
    }

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

    #[test]
    fn test_tool_mode_enum_equality() {
        assert_eq!(ToolMode::Compact, ToolMode::Compact);
        assert_ne!(ToolMode::Compact, ToolMode::Full);
    }

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

    #[test]
    fn test_append_workflow_tools_adds_five() {
        let mut tools = Vec::new();
        append_workflow_tools(&mut tools);
        assert_eq!(tools.len(), 5);
        assert_eq!(tools[0]["name"], "workflow_standup_report");
        assert_eq!(tools[4]["name"], "workflow_file_announce");
    }

    #[test]
    fn test_append_helper_tools_gmail_adds_send_and_reply() {
        let services = vec!["gmail".to_string()];
        let mut tools = Vec::new();
        append_helper_tools(&services, &mut tools);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "gmail_send");
        assert_eq!(tools[1]["name"], "gmail_reply");

        let schema = &tools[0]["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        let required_strs: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required_strs.contains(&"to"));
        assert!(required_strs.contains(&"subject"));
        assert!(required_strs.contains(&"body"));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("cc"));
        assert!(props.contains_key("bcc"));
    }

    #[test]
    fn test_append_helper_tools_no_gmail_adds_nothing() {
        let services = vec!["drive".to_string(), "calendar".to_string()];
        let mut tools = Vec::new();
        append_helper_tools(&services, &mut tools);
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_handle_gmail_send_missing_to() {
        let args = json!({"subject": "Hi", "body": "Hello"});
        let result = handle_gmail_send(&args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("'to'"));
    }

    #[tokio::test]
    async fn test_handle_gmail_send_missing_subject() {
        let args = json!({"to": "a@b.com", "body": "Hello"});
        let result = handle_gmail_send(&args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("'subject'"));
    }

    #[tokio::test]
    async fn test_handle_gmail_send_missing_body() {
        let args = json!({"to": "a@b.com", "subject": "Hi"});
        let result = handle_gmail_send(&args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("'body'"));
    }

    #[tokio::test]
    async fn test_gmail_send_rejected_when_helpers_disabled() {
        let config = ServerConfig {
            services: vec!["gmail".to_string()],
            workflows: false,
            helpers: false,
            tool_mode: ToolMode::Full,
        };
        let params = json!({
            "name": "gmail_send",
            "arguments": {"to": "a@b.com", "subject": "Hi", "body": "Hello"}
        });
        let result = handle_tools_call(&params, &config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--helpers"));
    }

    // --- gmail_reply tests ---

    #[test]
    fn test_append_helper_tools_gmail_reply_schema() {
        let services = vec!["gmail".to_string()];
        let mut tools = Vec::new();
        append_helper_tools(&services, &mut tools);

        let reply_tool = tools.iter().find(|t| t["name"] == "gmail_reply").unwrap();
        let schema = &reply_tool["inputSchema"];
        let required = schema["required"].as_array().unwrap();
        let required_strs: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required_strs.contains(&"message_id"));
        assert!(required_strs.contains(&"body"));

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("reply_all"));
        assert!(props.contains_key("cc"));
        assert!(props.contains_key("bcc"));
    }

    #[tokio::test]
    async fn test_handle_gmail_reply_missing_message_id() {
        let args = json!({"body": "Thanks!"});
        let result = handle_gmail_reply(&args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("'message_id'"));
    }

    #[tokio::test]
    async fn test_handle_gmail_reply_missing_body() {
        let args = json!({"message_id": "abc123"});
        let result = handle_gmail_reply(&args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("'body'"));
    }

    #[tokio::test]
    async fn test_gmail_reply_rejected_when_helpers_disabled() {
        let config = ServerConfig {
            services: vec!["gmail".to_string()],
            workflows: false,
            helpers: false,
            tool_mode: ToolMode::Full,
        };
        let params = json!({
            "name": "gmail_reply",
            "arguments": {"message_id": "abc123", "body": "Thanks!"}
        });
        let result = handle_tools_call(&params, &config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--helpers"));
    }

    // --- Common utility tests ---

    #[test]
    fn test_get_required_str_present() {
        let args = json!({"name": "test"});
        assert_eq!(get_required_str(&args, "name").unwrap(), "test");
    }

    #[test]
    fn test_get_required_str_missing() {
        let args = json!({});
        assert!(get_required_str(&args, "name").is_err());
    }

    #[test]
    fn test_get_required_str_empty() {
        let args = json!({"name": "  "});
        assert!(get_required_str(&args, "name").is_err());
    }

    #[test]
    fn test_get_optional_str_present() {
        let args = json!({"cc": "a@b.com"});
        assert_eq!(get_optional_str(&args, "cc"), Some("a@b.com"));
    }

    #[test]
    fn test_get_optional_str_missing() {
        let args = json!({});
        assert_eq!(get_optional_str(&args, "cc"), None);
    }

    #[test]
    fn test_get_optional_str_empty() {
        let args = json!({"cc": ""});
        assert_eq!(get_optional_str(&args, "cc"), None);
    }
}

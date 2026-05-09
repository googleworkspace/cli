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

//! MCP Streamable HTTP transport (Phase 1 — no authentication).
//!
//! Serves the same tool set as the stdio transport over HTTP on `POST /mcp`.
//! Start with: `gws mcp -s gmail --transport http --port 3000`

use std::sync::Arc;

use rmcp::transport::{
    streamable_http_server::session::local::LocalSessionManager, StreamableHttpServerConfig,
    StreamableHttpService,
};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, Implementation, ListToolsResult,
        PaginatedRequestParams, ServerInfo, Tool,
    },
    service::RequestContext,
    ErrorData as McpError, RoleServer, ServerHandler,
};
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
    error::GwsError,
    mcp_server::{build_tools_list, handle_tools_call, ServerConfig},
};

struct GwsMcpHandler {
    config: Arc<ServerConfig>,
    tools_cache: Arc<Mutex<Option<Vec<Tool>>>>,
}

impl ServerHandler for GwsMcpHandler {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info = Implementation::new("gws-mcp", env!("CARGO_PKG_VERSION"));
        info.capabilities.tools = Some(Default::default());
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut cache = self.tools_cache.lock().await;
        if cache.is_none() {
            let values = build_tools_list(&self.config)
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            let tools: Vec<Tool> = values
                .into_iter()
                .filter_map(|v| {
                    serde_json::from_value(v.clone())
                        .map_err(|e| {
                            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            eprintln!("[gws mcp] Warning: skipping tool '{name}': {e}");
                        })
                        .ok()
                })
                .collect();
            *cache = Some(tools);
        }
        Ok(ListToolsResult {
            tools: cache.as_ref().unwrap().clone(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let params = json!({
            "name": request.name.as_ref(),
            "arguments": request.arguments.unwrap_or_default()
        });
        let result = handle_tools_call(&params, &self.config)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        serde_json::from_value(result).map_err(|e| McpError::internal_error(e.to_string(), None))
    }
}

pub(crate) async fn start_http(
    config: ServerConfig,
    port: u16,
    bind: String,
) -> Result<(), GwsError> {
    let config = Arc::new(config);
    let tools_cache: Arc<Mutex<Option<Vec<Tool>>>> = Arc::new(Mutex::new(None));

    // rmcp default allowed_hosts already restricts to localhost/127.0.0.1/::1.
    // For external bind addresses the caller must use --bind 0.0.0.0 explicitly,
    // so we widen allowed_hosts only when the bind address is not loopback.
    let loopback = matches!(bind.as_str(), "127.0.0.1" | "::1" | "localhost");
    let http_config = if loopback {
        StreamableHttpServerConfig::default()
    } else {
        StreamableHttpServerConfig::default().disable_allowed_hosts()
    };

    let service: StreamableHttpService<GwsMcpHandler, LocalSessionManager> = {
        let config = config.clone();
        let tools_cache = tools_cache.clone();
        StreamableHttpService::new(
            move || {
                Ok(GwsMcpHandler {
                    config: config.clone(),
                    tools_cache: tools_cache.clone(),
                })
            },
            Default::default(),
            http_config,
        )
    };

    let app = axum::Router::new().nest_service("/mcp", service);
    let addr = format!("{bind}:{port}");
    eprintln!("[gws mcp] HTTP server listening on http://{bind}:{port}/mcp");
    if !loopback {
        eprintln!("[gws mcp] Warning: server is accessible from external hosts (--bind {bind})");
    }

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to bind to {addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("HTTP server error: {e}")))?;

    Ok(())
}

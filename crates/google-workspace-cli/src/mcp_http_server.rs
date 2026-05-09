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

//! MCP Streamable HTTP transport with optional OAuth2 PKCE authentication.
//!
//! Phase 1 (no auth): `gws mcp -s gmail --transport http --port 3000`
//! Phase 2 (OAuth2):  `gws mcp -s gmail --transport http --port 3000 --auth`
//!
//! When `--auth` is enabled the server implements the MCP Authorization spec
//! (2025-11-25, RFC 9728 / RFC 8414 / PKCE-S256) and acts as both OAuth2
//! Authorization Server and Resource Server.  Google is used as the identity
//! provider; a `client_secret.json` created via `gws auth setup` is required.
//!
//! Phase 2+3 limitation: Google API calls still use the shared credential from
//! `gws auth login`.  Per-user token isolation is Phase 4.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::{
    extract::{Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Json, Router,
};
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
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    error::GwsError,
    mcp_server::{build_tools_list, handle_tools_call, ServerConfig},
    oauth_config::{load_client_config, InstalledConfig},
};

// ─── Constants ───────────────────────────────────────────────────────────────

const SESSION_TTL: Duration = Duration::from_secs(8 * 3600); // 8 h
const PENDING_TTL: Duration = Duration::from_secs(600); // 10 min

// ─── Phase 1: MCP handler ────────────────────────────────────────────────────

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

// ─── Phase 2: Auth types ─────────────────────────────────────────────────────

/// State stored between `/oauth/authorize` and `/oauth/callback`.
struct PendingAuth {
    code_challenge: String,
    #[allow(dead_code)] // kept for future client_id validation
    client_id: String,
    redirect_uri: String,
    created_at: SystemTime,
}

/// State stored between `/oauth/callback` and `/oauth/token`.
struct PendingCode {
    code_challenge: String,
    client_redirect_uri: String,
    email: String,
    created_at: SystemTime,
}

struct UserSession {
    #[allow(dead_code)] // used in Phase 4 for per-user token lookup
    email: String,
    #[allow(dead_code)]
    created_at: SystemTime,
}

#[derive(Default)]
struct AuthStore {
    /// OAuth `state` value → PendingAuth (before Google callback)
    pending: Mutex<HashMap<String, PendingAuth>>,
    /// Short-lived auth code → PendingCode (after Google callback, before token exchange)
    codes: Mutex<HashMap<String, PendingCode>>,
    /// Bearer UUID → UserSession (live sessions)
    sessions: Mutex<HashMap<String, UserSession>>,
}

#[derive(Clone)]
struct AppState {
    auth_store: Arc<AuthStore>,
    oauth_cfg: Arc<InstalledConfig>,
    port: u16,
    bind: String,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn is_expired(created_at: SystemTime, ttl: Duration) -> bool {
    SystemTime::now()
        .duration_since(created_at)
        .map(|d| d >= ttl)
        .unwrap_or(true)
}

fn base64url(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(bytes)
}

fn verify_pkce_s256(verifier: &str, challenge: &str) -> bool {
    base64url(&Sha256::digest(verifier.as_bytes())) == challenge
}

fn urlencode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// Returns the public base URL of the server (uses `localhost` even when bound to `0.0.0.0`).
fn server_base(port: u16, bind: &str) -> String {
    let host = match bind {
        "0.0.0.0" => "localhost",
        "::" => "::1",
        other => other,
    };
    format!("http://{host}:{port}")
}

fn build_google_auth_url(
    client_id: &str,
    redirect_uri: &str,
    scopes: &str,
    state: &str,
) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/auth?\
         scope={}&access_type=offline&redirect_uri={}&\
         response_type=code&client_id={}&state={}&\
         prompt=select_account+consent",
        urlencode(scopes),
        urlencode(redirect_uri),
        urlencode(client_id),
        urlencode(state),
    )
}

// ─── Phase 3: Bearer middleware ───────────────────────────────────────────────

async fn bearer_auth_middleware(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();
    // OAuth and well-known endpoints are public
    if path.starts_with("/oauth/") || path.starts_with("/.well-known/") {
        return next.run(request).await;
    }

    let Some(token) = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    else {
        return (
            StatusCode::UNAUTHORIZED,
            [(
                "WWW-Authenticate",
                "Bearer realm=\"gws-mcp\", resource_metadata=\"/.well-known/oauth-protected-resource\"",
            )],
        )
            .into_response();
    };

    let sessions = state.auth_store.sessions.lock().await;
    match sessions.get(&token) {
        Some(s) if !is_expired(s.created_at, SESSION_TTL) => {}
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                [(
                    "WWW-Authenticate",
                    "Bearer realm=\"gws-mcp\", error=\"invalid_token\", resource_metadata=\"/.well-known/oauth-protected-resource\"",
                )],
            )
                .into_response();
        }
    }

    next.run(request).await
}

// ─── OAuth metadata endpoints ────────────────────────────────────────────────

/// RFC 9728 — OAuth 2.0 Protected Resource Metadata
async fn protected_resource_metadata(State(state): State<AppState>) -> Json<Value> {
    let base = server_base(state.port, &state.bind);
    Json(json!({
        "resource": base,
        "authorization_servers": [base],
        "bearer_methods_supported": ["header"],
        "resource_documentation": "https://github.com/shigechika/gws-mcp"
    }))
}

/// RFC 8414 — OAuth 2.0 Authorization Server Metadata
async fn authorization_server_metadata(State(state): State<AppState>) -> Json<Value> {
    let base = server_base(state.port, &state.bind);
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/oauth/token"),
        "registration_endpoint": format!("{base}/oauth/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["openid", "email", "profile"]
    }))
}

// ─── Dynamic Client Registration stub (RFC 7591) ─────────────────────────────

#[derive(Deserialize)]
struct RegisterRequest {
    client_name: Option<String>,
    redirect_uris: Option<Vec<String>>,
}

async fn oauth_register(
    State(_): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    let client_id = req
        .client_name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "gws-mcp-client".to_string());
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": client_id,
            "redirect_uris": req.redirect_uris.unwrap_or_default(),
            "grant_types": ["authorization_code"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none"
        })),
    )
}

// ─── /oauth/authorize ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AuthorizeParams {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
    scope: Option<String>,
}

async fn oauth_authorize(
    State(state): State<AppState>,
    Query(p): Query<AuthorizeParams>,
) -> Result<Redirect, (StatusCode, String)> {
    if p.response_type != "code" {
        return Err((
            StatusCode::BAD_REQUEST,
            "unsupported_response_type".to_string(),
        ));
    }
    if p.code_challenge_method != "S256" {
        return Err((
            StatusCode::BAD_REQUEST,
            "only S256 code_challenge_method is supported".to_string(),
        ));
    }

    state.auth_store.pending.lock().await.insert(
        p.state.clone(),
        PendingAuth {
            code_challenge: p.code_challenge,
            client_id: p.client_id,
            redirect_uri: p.redirect_uri,
            created_at: SystemTime::now(),
        },
    );

    let callback = format!("{}/oauth/callback", server_base(state.port, &state.bind));
    // Request openid/email/profile from Google to identify the user.
    // GWS API scopes are not needed here because Phase 2+3 still uses the
    // shared credential from `gws auth login`.  Per-user GWS tokens are Phase 4.
    let scopes = p.scope.as_deref().unwrap_or("openid email profile");
    let google_url =
        build_google_auth_url(&state.oauth_cfg.client_id, &callback, scopes, &p.state);

    Ok(Redirect::to(&google_url))
}

// ─── /oauth/callback ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: String,
    error: Option<String>,
}

async fn oauth_callback(
    State(state): State<AppState>,
    Query(p): Query<CallbackParams>,
) -> Result<Redirect, (StatusCode, String)> {
    if let Some(err) = p.error {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Google OAuth error: {err}"),
        ));
    }
    let google_code = p
        .code
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing code".to_string()))?;

    let pending = {
        let mut lock = state.auth_store.pending.lock().await;
        lock.remove(&p.state)
            .filter(|pa| !is_expired(pa.created_at, PENDING_TTL))
            .ok_or_else(|| (StatusCode::BAD_REQUEST, "invalid or expired state".to_string()))?
    };

    let callback = format!("{}/oauth/callback", server_base(state.port, &state.bind));
    let token_resp =
        exchange_google_code(&state.oauth_cfg, &google_code, &callback)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let email = get_google_email(&token_resp.access_token)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let auth_code = Uuid::new_v4().to_string();
    state.auth_store.codes.lock().await.insert(
        auth_code.clone(),
        PendingCode {
            code_challenge: pending.code_challenge,
            client_redirect_uri: pending.redirect_uri.clone(),
            email,
            created_at: SystemTime::now(),
        },
    );

    let location = format!(
        "{}?code={}&state={}",
        pending.redirect_uri,
        urlencode(&auth_code),
        urlencode(&p.state),
    );
    Ok(Redirect::to(&location))
}

// ─── /oauth/token ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    code: String,
    code_verifier: String,
    redirect_uri: Option<String>,
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
}

async fn oauth_token(
    State(state): State<AppState>,
    Form(req): Form<TokenRequest>,
) -> Result<Json<TokenResponse>, (StatusCode, Json<Value>)> {
    if req.grant_type != "authorization_code" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unsupported_grant_type"})),
        ));
    }

    let pending_code = {
        let mut lock = state.auth_store.codes.lock().await;
        lock.remove(&req.code)
            .filter(|pc| !is_expired(pc.created_at, PENDING_TTL))
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_grant"})),
                )
            })?
    };

    // Validate redirect_uri if provided
    if let Some(uri) = &req.redirect_uri {
        if *uri != pending_code.client_redirect_uri {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid_grant", "error_description": "redirect_uri mismatch"})),
            ));
        }
    }

    // Verify PKCE S256
    if !verify_pkce_s256(&req.code_verifier, &pending_code.code_challenge) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant", "error_description": "PKCE verification failed"})),
        ));
    }

    let bearer = Uuid::new_v4().to_string();
    state.auth_store.sessions.lock().await.insert(
        bearer.clone(),
        UserSession {
            email: pending_code.email.clone(),
            created_at: SystemTime::now(),
        },
    );

    eprintln!("[gws mcp] Session created for {}", pending_code.email);

    Ok(Json(TokenResponse {
        access_token: bearer,
        token_type: "bearer",
        expires_in: SESSION_TTL.as_secs(),
    }))
}

// ─── Google helpers ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GoogleTokenResp {
    access_token: String,
}

async fn exchange_google_code(
    cfg: &InstalledConfig,
    code: &str,
    redirect_uri: &str,
) -> anyhow::Result<GoogleTokenResp> {
    let client = crate::client::shared_client()?;
    let params = [
        ("client_id", cfg.client_id.as_str()),
        ("client_secret", cfg.client_secret.as_str()),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = crate::auth::response_text_or_placeholder(resp.text().await);
        anyhow::bail!("Google token exchange failed ({status}): {body}");
    }
    resp.json().await.map_err(Into::into)
}

async fn get_google_email(access_token: &str) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct UserinfoResp {
        email: String,
    }
    let client = crate::client::shared_client()?;
    let resp: UserinfoResp = client
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(access_token)
        .send()
        .await?
        .json()
        .await?;
    Ok(resp.email)
}

// ─── Router builder ───────────────────────────────────────────────────────────

fn build_auth_router(
    mcp_service: StreamableHttpService<GwsMcpHandler, LocalSessionManager>,
    app_state: AppState,
) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route("/oauth/register", post(oauth_register))
        .route("/oauth/authorize", get(oauth_authorize))
        .route("/oauth/callback", get(oauth_callback))
        .route("/oauth/token", post(oauth_token))
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            bearer_auth_middleware,
        ))
        .with_state(app_state)
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub(crate) async fn start_http(
    config: ServerConfig,
    port: u16,
    bind: String,
    enable_auth: bool,
) -> Result<(), GwsError> {
    let config = Arc::new(config);
    let tools_cache: Arc<Mutex<Option<Vec<Tool>>>> = Arc::new(Mutex::new(None));

    let loopback = matches!(bind.as_str(), "127.0.0.1" | "::1" | "localhost");
    let http_config = if loopback {
        StreamableHttpServerConfig::default()
    } else {
        StreamableHttpServerConfig::default().disable_allowed_hosts()
    };

    let mcp_service: StreamableHttpService<GwsMcpHandler, LocalSessionManager> = {
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

    let app = if enable_auth {
        let oauth_cfg = load_client_config().map_err(|e| {
            GwsError::Other(anyhow::anyhow!(
                "--auth requires client_secret.json (run `gws auth setup` first): {e}"
            ))
        })?;
        let app_state = AppState {
            auth_store: Arc::new(AuthStore::default()),
            oauth_cfg: Arc::new(oauth_cfg),
            port,
            bind: bind.clone(),
        };
        build_auth_router(mcp_service, app_state)
    } else {
        Router::new().nest_service("/mcp", mcp_service)
    };

    let addr = format!("{bind}:{port}");
    let base = server_base(port, &bind);
    eprintln!("[gws mcp] HTTP server listening on {base}/mcp");
    if enable_auth {
        eprintln!("[gws mcp] OAuth2 auth enabled — callback URL: {base}/oauth/callback");
        eprintln!(
            "[gws mcp] Ensure {base}/oauth/callback is registered as a redirect URI in Google Cloud Console"
        );
    }
    if !loopback {
        eprintln!("[gws mcp] Warning: server accessible from external hosts (--bind {bind})");
    }

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to bind to {addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("HTTP server error: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_s256_rfc7636_example() {
        // RFC 7636 §4.6 worked example
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(verify_pkce_s256(verifier, challenge));
    }

    #[test]
    fn pkce_s256_rejects_wrong_verifier() {
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(!verify_pkce_s256("wrong-verifier", challenge));
    }
}

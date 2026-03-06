# Deploying the gws MCP Server

## What is this?

`gws` is a CLI that exposes Google Workspace APIs (Gmail, Calendar, Drive, Docs, Sheets, Slides) as [MCP](https://modelcontextprotocol.io/) tools. The HTTP+SSE transport mode lets it run as a shared server so multiple team members can connect to it remotely from AI agents, CLI tools, or any MCP client.

```
MCP Client ──HTTP──> gws MCP server ──OAuth──> Google Workspace APIs
(Claude, etc.)       (this service)            (Gmail, Drive, etc.)
```

The server is **stateless** — sessions live in memory and expire after 30 minutes. No database or persistent storage is needed.

---

## Decisions Before Deploying

| Decision | Options | Recommendation |
|----------|---------|----------------|
| **Deployment target** | Cloud Run, Compute Engine VM, GKE, Docker on any host, bare metal | Cloud Run for auto-scaling; GCE VM for simplicity |
| **Auth model** | Per-user bearer tokens (each user sends their own Google OAuth token) | Per-user tokens — no shared credentials on the server |
| **Network access** | Internal only (VPN/IAP), Cloud Run with IAM, fully public | Internal or IAM-gated — do not expose publicly without auth |
| **Google APIs to expose** | `all` (default) exposes all 17 supported services, or pick specific ones: `drive`, `gmail`, `calendar`, `sheets`, `docs`, `slides`, `tasks`, `people`, `chat`, `classroom`, `forms`, `keep`, `meet`, `admin-reports`, `events`, `modelarmor`, `workflow` | `all` — let users access everything |

---

## Requirements

- **Docker** (for containerized deployments) or **Rust 1.83+** (for building from source)
- A **Google Cloud project** with the relevant APIs enabled (enable all that your team will use):
  - Gmail API, Google Calendar API, Google Drive API
  - Google Sheets API, Google Docs API, Google Slides API
  - Google Tasks API, People API, Google Chat API
  - Google Classroom API, Google Forms API, Google Keep API
  - Google Meet API, Admin SDK (Reports), Workspace Events API
- **OAuth credentials** — each user obtains their own access token (e.g. via `gcloud auth print-access-token` or your org's OAuth flow)
- No persistent storage, no database, no Redis

---

## Configuration Reference

### CLI Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--transport` | `stdio` | Transport mode: `stdio` (local) or `sse` (HTTP server) |
| `--port` | `8080` | Port to listen on (only with `--transport sse`) |
| `--host` | `127.0.0.1` | Bind address (use `0.0.0.0` for containerized/remote deployments) |
| `-s`, `--services` | *(none)* | Services to expose: `all` for everything, or comma-separated list (e.g. `drive,gmail,calendar`) |
| `--tool-mode` | `full` | `full` = one tool per API method; `compact` = one tool per service + discovery tool |
| `--workflows` | `false` | Expose cross-service workflow tools (standup report, meeting prep, etc.) |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `GOOGLE_WORKSPACE_CLI_TOKEN` | Pre-obtained OAuth2 access token (highest priority; bypasses credential file loading) |
| `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` | Path to OAuth credentials JSON file |
| `GOOGLE_APPLICATION_CREDENTIALS` | Standard Google ADC path (fallback) |
| `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` | Override config directory (default: `~/.config/gws`) |
| `GOOGLE_WORKSPACE_CLI_CLIENT_ID` | OAuth client ID (for `gws auth login`) |
| `GOOGLE_WORKSPACE_CLI_CLIENT_SECRET` | OAuth client secret (paired with client ID) |

---

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check — returns `{"status":"ok"}` |
| `GET` | `/sse` | SSE stream (legacy MCP transport, creates a session) |
| `POST` | `/mcp` | JSON-RPC handler (Streamable HTTP transport) |
| `DELETE` | `/mcp` | Terminate a session |

- Sessions expire after **30 minutes** of inactivity
- Cleanup runs every **60 seconds**
- Session IDs are passed via `Mcp-Session-Id` header or `?sessionId=` query param

---

## Deployment Options

### Option A: Cloud Run (managed, auto-scaling)

**Best for:** Production deployments with variable load, zero ops overhead.

#### 1. Enable APIs

```bash
gcloud config set project YOUR_PROJECT_ID

gcloud services enable \
  cloudbuild.googleapis.com \
  run.googleapis.com \
  containerregistry.googleapis.com \
  gmail.googleapis.com \
  calendar-json.googleapis.com \
  drive.googleapis.com \
  sheets.googleapis.com \
  docs.googleapis.com \
  slides.googleapis.com
```

#### 2. Deploy

```bash
# One-command build + deploy
gcloud builds submit --config cloudbuild.yaml \
  --substitutions=_REGION=us-central1,_SERVICE_NAME=gws-mcp
```

#### 3. Restrict access (important)

The included `cloudbuild.yaml` deploys with `--no-allow-unauthenticated`. Grant access to specific users or service accounts:

```bash
# Grant a specific user
gcloud run services add-iam-policy-binding gws-mcp \
  --region=us-central1 \
  --member="user:alice@yourcompany.com" \
  --role="roles/run.invoker"

# Or grant an entire group
gcloud run services add-iam-policy-binding gws-mcp \
  --region=us-central1 \
  --member="group:engineering@yourcompany.com" \
  --role="roles/run.invoker"
```

Users then authenticate to Cloud Run with:
```bash
curl -H "Authorization: Bearer $(gcloud auth print-identity-token)" \
  https://gws-mcp-XXXX-uc.a.run.app/health
```

**Note:** Cloud Run IAM authentication uses *identity tokens* to reach the service. The Google Workspace *access token* (for calling Gmail, Drive, etc.) is sent separately in the MCP request's `Authorization: Bearer` header.

---

### Option B: Compute Engine VM (simple, always-on)

**Best for:** Small teams, internal use behind VPN, simple always-on instance.

#### 1. Create a VM

```bash
gcloud compute instances create gws-mcp \
  --zone=us-central1-a \
  --machine-type=e2-small \
  --image-family=debian-12 \
  --image-project=debian-cloud \
  --tags=gws-mcp
```

#### 2. Install and run (Docker)

SSH into the VM, then:

```bash
# Install Docker
sudo apt-get update && sudo apt-get install -y docker.io

# Clone and build
git clone https://github.com/YOUR_ORG/cli.git
cd cli
sudo docker build -t gws-mcp .

# Run
sudo docker run -d --name gws-mcp --restart=always -p 8080:8080 gws-mcp
```

#### 3. Or run (bare binary)

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Build
git clone https://github.com/YOUR_ORG/cli.git
cd cli
cargo build --profile dist

# Run
./target/dist/gws mcp --transport sse --host 0.0.0.0 --port 8080 \
  -s all
```

#### 4. Systemd service (for bare binary)

Create `/etc/systemd/system/gws-mcp.service`:

```ini
[Unit]
Description=gws MCP Server
After=network.target

[Service]
Type=simple
User=gws
ExecStart=/usr/local/bin/gws mcp --transport sse --host 0.0.0.0 --port 8080 -s all
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable gws-mcp
sudo systemctl start gws-mcp
```

#### 5. Firewall (restrict to internal network)

```bash
# Allow only from internal IP ranges
gcloud compute firewall-rules create allow-gws-mcp \
  --allow=tcp:8080 \
  --target-tags=gws-mcp \
  --source-ranges=10.0.0.0/8
```

---

### Option C: Docker on any host

**Best for:** On-prem servers, AWS/Azure, or any machine with Docker.

```bash
# Build
docker build -t gws-mcp .

# Run
docker run -d --name gws-mcp --restart=always -p 8080:8080 gws-mcp
```

#### Docker Compose

```yaml
# docker-compose.yml
services:
  gws-mcp:
    build: .
    ports:
      - "8080:8080"
    restart: always
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s
      timeout: 5s
      retries: 3
```

```bash
docker compose up -d
```

#### TLS with Caddy (reverse proxy)

```
# Caddyfile
gws-mcp.yourcompany.com {
    reverse_proxy localhost:8080
}
```

Caddy automatically provisions TLS certificates via Let's Encrypt.

---

### Option D: Bare metal (no Docker)

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build optimized binary
cargo build --profile dist

# Copy to a standard location
sudo cp target/dist/gws /usr/local/bin/gws

# Run
gws mcp --transport sse --host 0.0.0.0 --port 8080 \
  -s all
```

Only runtime dependency is `ca-certificates` (for TLS to Google APIs).

---

## Security Recommendations

1. **Do not expose publicly without authentication.** Use Cloud Run IAM, a VPN, or Identity-Aware Proxy (IAP).
2. **Use TLS.** Cloud Run handles this automatically. For VMs, put a reverse proxy (Caddy, nginx) in front.
3. **Per-user tokens.** Each user sends their own Google OAuth token in the `Authorization: Bearer` header. No shared service account credentials are stored on the server.
4. **Restrict services.** Only expose the Google APIs your team actually needs via the `-s` flag.

---

## Verifying the Deployment

### 1. Health check

```bash
curl https://YOUR_HOST/health
# Expected: {"status":"ok"}
```

### 2. Initialize a session

```bash
curl -X POST https://YOUR_HOST/mcp \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $(gcloud auth print-access-token)" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
```

Save the `Mcp-Session-Id` response header for subsequent requests.

### 3. List available tools

```bash
curl -X POST https://YOUR_HOST/mcp \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $(gcloud auth print-access-token)" \
  -H "Mcp-Session-Id: SESSION_ID_FROM_STEP_2" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
```

You should see tools for each enabled service (e.g., `drive_files_list`, `gmail_users_messages_get`, etc.).

---

## Connecting MCP Clients

### Claude Desktop / Claude Code

Add to your MCP client config (e.g., `~/.claude/mcp.json` or Claude Desktop settings):

```json
{
  "mcpServers": {
    "google-workspace": {
      "url": "https://YOUR_HOST/sse",
      "headers": {
        "Authorization": "Bearer YOUR_GOOGLE_ACCESS_TOKEN"
      }
    }
  }
}
```

### Any MCP Client (Streamable HTTP)

Point the client at `https://YOUR_HOST/mcp` with:
- `Content-Type: application/json`
- `Authorization: Bearer <google-access-token>`

The client sends `initialize` first, then uses the returned `Mcp-Session-Id` header for all subsequent requests.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `{"status":"ok"}` on `/health` but MCP calls fail | Missing or expired OAuth token | Run `gcloud auth print-access-token` to get a fresh token |
| `403 Forbidden` on Cloud Run | User not granted `roles/run.invoker` | Add IAM binding (see Option A step 3) |
| `Connection refused` | Server not running or wrong port | Check `docker ps` or `systemctl status gws-mcp` |
| `No tools returned` from `tools/list` | No services configured | Ensure `-s drive,gmail,...` flag is set |
| Google API returns `403` | API not enabled in GCP project | Enable the specific API in Cloud Console |
| `Authentication failed` in server logs | Token expired or wrong scopes | Get a new token with appropriate scopes |

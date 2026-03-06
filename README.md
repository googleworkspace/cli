<h1 align="center">gws</h1>

**One CLI for all of Google Workspace — built for humans and AI agents.**<br>
Drive, Gmail, Calendar, and every Workspace API. Zero boilerplate. Structured JSON output. 40+ agent skills included.

> [!NOTE]
> This is **not** an officially supported Google product.

<p>
  <a href="https://www.npmjs.com/package/@googleworkspace/cli"><img src="https://img.shields.io/npm/v/@googleworkspace/cli" alt="npm version"></a>
  <a href="https://github.com/googleworkspace/cli/blob/main/LICENSE"><img src="https://img.shields.io/github/license/googleworkspace/cli" alt="license"></a>
  <a href="https://github.com/googleworkspace/cli/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/googleworkspace/cli/ci.yml?branch=main&label=CI" alt="CI status"></a>
  <a href="https://www.npmjs.com/package/@googleworkspace/cli"><img src="https://img.shields.io/npm/unpacked-size/@googleworkspace/cli" alt="install size"></a>
</p>
<br>

```bash
npm install -g @googleworkspace/cli
```

`gws` doesn't ship a static list of commands. It reads Google's own [Discovery Service](https://developers.google.com/discovery) at runtime and builds its entire command surface dynamically. When Google Workspace adds an API endpoint or method, `gws` picks it up automatically.

> [!IMPORTANT]
> This project is under active development. Expect breaking changes as we march toward v1.0.

## Contents

- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Why gws?](#why-gws)
- [Authentication](#authentication)
- [AI Agent Skills](#ai-agent-skills)
- [MCP Server](#mcp-server)
- [Advanced Usage](#advanced-usage)
- [Environment Variables](#environment-variables)
- [Architecture](#architecture)
- [Troubleshooting](#troubleshooting)
- [Development](#development)

## Prerequisites

- **Node.js 18+** — for `npm install` (or download a pre-built binary from [GitHub Releases](https://github.com/googleworkspace/cli/releases))
- **A Google Cloud project** — required for OAuth credentials. You can create one via the [Google Cloud Console](https://console.cloud.google.com/) or with the [`gcloud` CLI](https://cloud.google.com/sdk/docs/install) or with the `gws auth setup` command.
- **A Google account** with access to Google Workspace

## Installation

```bash
npm install -g @googleworkspace/cli
```

> The npm package bundles pre-built native binaries for your OS and architecture.
> No Rust toolchain required.

Pre-built binaries are also available on the [GitHub Releases](https://github.com/googleworkspace/cli/releases) page.

Or build from source:

```bash
cargo install --git https://github.com/googleworkspace/cli --locked
```

A Nix flake is also available at `github:googleworkspace/cli`

```bash
nix run github:googleworkspace/cli
```

## Quick Start

```bash
gws auth setup     # walks you through Google Cloud project config
gws auth login     # subsequent OAuth login
gws drive files list --params '{"pageSize": 5}'
```

## Why gws?

**For humans** — stop writing `curl` calls against REST docs. `gws` gives you `--help` on every resource, `--dry-run` to preview requests, and auto‑pagination.

**For AI agents** — every response is structured JSON. Pair it with the included agent skills and your LLM can manage Workspace without custom tooling.

```bash
# List the 10 most recent files
gws drive files list --params '{"pageSize": 10}'

# Create a spreadsheet
gws sheets spreadsheets create --json '{"properties": {"title": "Q1 Budget"}}'

# Send a Chat message
gws chat spaces messages create \
  --params '{"parent": "spaces/xyz"}' \
  --json '{"text": "Deploy complete."}' \
  --dry-run

# Introspect any method's request/response schema
gws schema drive.files.list

# Stream paginated results as NDJSON
gws drive files list --params '{"pageSize": 100}' --page-all | jq -r '.files[].name'
```

## Authentication

The CLI supports multiple auth workflows so it works on your laptop, in CI, and on a server.

### Which setup should I use?

| I have… | Use |
|---|---|
| `gcloud` installed and authenticated | [`gws auth setup`](#interactive-local-desktop) (fastest) |
| A GCP project but no `gcloud` | [Manual OAuth setup](#manual-oauth-setup-google-cloud-console) |
| An existing OAuth access token | [`GOOGLE_WORKSPACE_CLI_TOKEN`](#pre-obtained-access-token) |
| Existing Credentials | [`GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE`](#service-account-server-to-server) |

### Interactive (local desktop)

Credentials are encrypted at rest (AES-256-GCM) with the key stored in your OS keyring.

```bash
gws auth setup       # one-time: creates a Cloud project, enables APIs, logs you in
gws auth login       # subsequent scope selection and login
```

> `gws auth setup` requires the [`gcloud` CLI](https://cloud.google.com/sdk/docs/install). If you don't have `gcloud`, use the [manual setup](#manual-oauth-setup-google-cloud-console) below instead.

> [!WARNING]
> **Scope limits in testing mode:** If your OAuth app is unverified (testing mode),
> Google limits consent to ~25 scopes. The `recommended` scope preset includes 85+
> scopes and **will fail** for unverified apps (especially for `@gmail.com` accounts).
> Choose individual services instead to filter the scope picker:
> ```bash
> gws auth login -s drive,gmail,sheets
> ```

### Multiple accounts

You can authenticate with more than one Google account and switch between them:

```bash
gws auth login --account work@corp.com  # login and register an account
gws auth login --account personal@gmail.com

gws auth list  # list registered accounts
gws auth default work@corp.com. # set the default

gws --account personal@gmail.com drive files list  # one-off override
export GOOGLE_WORKSPACE_CLI_ACCOUNT=personal@gmail.com  # env var override
```

Credentials are stored per-account as `credentials.<b64-email>.enc` in `~/.config/gws/`, with an `accounts.json` registry tracking defaults.

### Manual OAuth setup (Google Cloud Console)

Use this when `gws auth setup` cannot automate project/client creation, or when you want explicit control.

1. Open Google Cloud Console in the target project:
   - OAuth consent screen: `https://console.cloud.google.com/apis/credentials/consent?project=<PROJECT_ID>`
   - Credentials: `https://console.cloud.google.com/apis/credentials?project=<PROJECT_ID>`
2. Configure OAuth branding/audience if prompted:
   - App type: **External** (testing mode is fine)
3. Add your account under **Test users**
4. Create an OAuth client:
   - Type: **Desktop app**
5. Download the client JSON and save it to:
   - `~/.config/gws/client_secret.json`

> [!IMPORTANT]
> **You must add yourself as a test user.** In the OAuth consent screen, click
> **Test users → Add users** and enter your Google account email. Without this,
> login will fail with a generic "Access blocked" error.

Then run:

```bash
gws auth login
```

### Browser-assisted auth (human or agent)

You can complete OAuth either manually or with browser automation.

- **Human flow**: run `gws auth login`, open the printed URL, approve scopes.
- **Agent-assisted flow**: the agent opens the URL, selects account, handles consent prompts, and returns control once the localhost callback succeeds.

If consent shows **"Google hasn't verified this app"** (testing mode), click **Continue**.
If scope checkboxes appear, select required scopes (or **Select all**) before continuing.

### Headless / CI (export flow)

1. Complete interactive auth on a machine with a browser.
2. Export credentials:
   ```bash
   gws auth export --unmasked > credentials.json
   ```
3. On the headless machine:
   ```bash
   export GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE=/path/to/credentials.json
   gws drive files list   # just works
   ```

### Service Account (server-to-server)

Point to your key file; no login needed.

```bash
export GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE=/path/to/service-account.json
gws drive files list
```

### Pre-obtained Access Token

Useful when another tool (e.g. `gcloud`) already mints tokens for your environment.

```bash
export GOOGLE_WORKSPACE_CLI_TOKEN=$(gcloud auth print-access-token)
```

### Precedence

| Priority | Source                            | Set via                                 |
| -------- | --------------------------------- | --------------------------------------- |
| 1        | Access token                      | `GOOGLE_WORKSPACE_CLI_TOKEN`            |
| 2        | Credentials file                  | `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` |
| 3        | Per-account encrypted credentials | `gws auth login --account EMAIL`        |
| 4        | Plaintext credentials             | `~/.config/gws/credentials.json`        |

Account resolution: `--account` flag > `GOOGLE_WORKSPACE_CLI_ACCOUNT` env var > default in `accounts.json`.

Environment variables can also live in a `.env` file.

## AI Agent Skills

The repo ships 100+ Agent Skills (`SKILL.md` files) — one for every supported API, plus higher-level helpers for common workflows and 50 curated recipes for Gmail, Drive, Docs, Calendar, and Sheets. See the full [Skills Index](docs/skills.md) for the complete list.

```bash
# Install all skills at once
npx skills add https://github.com/googleworkspace/cli

# Or pick only what you need
npx skills add https://github.com/googleworkspace/cli/tree/main/skills/gws-drive
npx skills add https://github.com/googleworkspace/cli/tree/main/skills/gws-gmail
```

<details>
<summary>OpenClaw setup</summary>

```bash
# Symlink all skills (stays in sync with repo)
ln -s $(pwd)/skills/gws-* ~/.openclaw/skills/

# Or copy specific skills
cp -r skills/gws-drive skills/gws-gmail ~/.openclaw/skills/
```

The `gws-shared` skill includes an `install` block so OpenClaw auto-installs the CLI via `npm` if `gws` isn't on PATH.

</details>

## Gemini CLI Extension

1. Authenticate the CLI first:

   ```bash
   gws auth setup
   ```

2. Install the extension into the Gemini CLI:
   ```bash
   gemini extensions install https://github.com/googleworkspace/cli
   ```

Installing this extension gives your Gemini CLI agent direct access to all `gws` commands and Google Workspace agent skills. Because `gws` handles its own authentication securely, you simply need to authenticate your terminal once prior to using the agent, and the extension will automatically inherit your credentials.

## MCP Server

`gws mcp` starts a [Model Context Protocol](https://modelcontextprotocol.io) server over stdio, exposing Google Workspace APIs as structured tools that any MCP-compatible client (Claude Desktop, Gemini CLI, VS Code, etc.) can call.

```bash
gws mcp -s drive                  # expose Drive tools
gws mcp -s drive,gmail,calendar   # expose multiple services
gws mcp -s all                    # expose all services (many tools!)
```

Configure in your MCP client:

```json
{
  "mcpServers": {
    "gws": {
      "command": "gws",
      "args": ["mcp", "-s", "drive,gmail,calendar"]
    }
  }
}
```

> [!TIP]
> Each service adds roughly 10–80 tools. Keep the list to what you actually need
> to stay under your client's tool limit (typically 50–100 tools).

| Flag                    | Description                                  |
| ----------------------- | -------------------------------------------- |
| `-s, --services <list>` | Comma-separated services to expose, or `all` |
| `-w, --workflows`       | Also expose workflow tools                   |
| `-e, --helpers`         | Also expose helper tools                     |

## Advanced Usage

### Multipart Uploads

```bash
gws drive files create --json '{"name": "report.pdf"}' --upload ./report.pdf
```

### Pagination

| Flag                | Description                                    | Default |
| ------------------- | ---------------------------------------------- | ------- |
| `--page-all`        | Auto-paginate, one JSON line per page (NDJSON) | off     |
| `--page-limit <N>`  | Max pages to fetch                             | 10      |
| `--page-delay <MS>` | Delay between pages                            | 100 ms  |

### Google Sheets — Shell Escaping

Sheets ranges use `!` which bash interprets as history expansion. Always wrap values in **single quotes**:

```bash
# Read cells A1:C10 from "Sheet1"
gws sheets spreadsheets values get \
  --params '{"spreadsheetId": "SPREADSHEET_ID", "range": "Sheet1!A1:C10"}'

# Append rows
gws sheets spreadsheets values append \
  --params '{"spreadsheetId": "ID", "range": "Sheet1!A1", "valueInputOption": "USER_ENTERED"}' \
  --json '{"values": [["Name", "Score"], ["Alice", 95]]}'
```

### Model Armor (Response Sanitization)

Integrate [Google Cloud Model Armor](https://cloud.google.com/security/products/model-armor) to scan API responses for prompt injection before they reach your agent.

```bash
gws gmail users messages get --params '...' \
  --sanitize "projects/P/locations/L/templates/T"
```

| Variable                                 | Description                  |
| ---------------------------------------- | ---------------------------- |
| `GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE` | Default Model Armor template |
| `GOOGLE_WORKSPACE_CLI_SANITIZE_MODE`     | `warn` (default) or `block`  |

## Environment Variables

All variables are optional. See [`.env.example`](.env.example) for a copy-paste template.

| Variable | Description |
|---|---|
| `GOOGLE_WORKSPACE_CLI_TOKEN` | Pre-obtained OAuth2 access token (highest priority) |
| `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` | Path to OAuth credentials JSON (user or service account) |
| `GOOGLE_WORKSPACE_CLI_ACCOUNT` | Default account email (overridden by `--account` flag) |
| `GOOGLE_WORKSPACE_CLI_CLIENT_ID` | OAuth client ID (alternative to `client_secret.json`) |
| `GOOGLE_WORKSPACE_CLI_CLIENT_SECRET` | OAuth client secret (paired with `CLIENT_ID`) |
| `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` | Override config directory (default: `~/.config/gws`) |
| `GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE` | Default Model Armor template |
| `GOOGLE_WORKSPACE_CLI_SANITIZE_MODE` | `warn` (default) or `block` |
| `GOOGLE_WORKSPACE_PROJECT_ID` | GCP project ID fallback for helper commands |

Environment variables can also be set in a `.env` file (loaded via [dotenvy](https://crates.io/crates/dotenvy)).

## Architecture

`gws` uses a **two-phase parsing** strategy:

1. Read `argv[1]` to identify the service (e.g. `drive`)
2. Fetch the service's Discovery Document (cached 24 h)
3. Build a `clap::Command` tree from the document's resources and methods
4. Re-parse the remaining arguments
5. Authenticate, build the HTTP request, execute

All output — success, errors, download metadata — is structured JSON.

## Troubleshooting

### "Access blocked" or 403 during login

Your OAuth app is in **testing mode** and your account is not listed as a test user.

**Fix:** Open the [OAuth consent screen](https://console.cloud.google.com/apis/credentials/consent) in your GCP project → **Test users** → **Add users** → enter your Google account email. Then retry `gws auth login`.

### "Google hasn't verified this app"

Expected when your app is in testing mode. Click **Advanced** → **Go to \<app name\> (unsafe)** to proceed. This is safe for personal use; verification is only required to publish the app to other users.

### Too many scopes / consent screen error

Unverified (testing mode) apps are limited to ~25 OAuth scopes. The `recommended` scope preset includes many scopes and will exceed this limit.

**Fix:** Select only the scopes you need:

```bash
gws auth login --scopes drive,gmail,calendar
```

### `gcloud` CLI not found

`gws auth setup` requires the `gcloud` CLI to automate project creation. You have three options:

1. [Install gcloud](https://cloud.google.com/sdk/docs/install) and use `gcloud` directly.
2. Re-run `gws auth setup` which wraps `gcloud` calls.
3. Skip `gcloud` entirely — set up OAuth credentials manually in the [Cloud Console](#manual-oauth-setup-google-cloud-console)

### `redirect_uri_mismatch`

The OAuth client was not created as a **Desktop app** type. In the [Credentials](https://console.cloud.google.com/apis/credentials) page, delete the existing client, create a new one with type **Desktop app**, and download the new JSON.

### API not enabled — `accessNotConfigured`

If a required Google API is not enabled for your GCP project, you will see a
403 error with reason `accessNotConfigured`:

```json
{
  "error": {
    "code": 403,
    "message": "Gmail API has not been used in project 549352339482 ...",
    "reason": "accessNotConfigured",
    "enable_url": "https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482"
  }
}
```

`gws` also prints an actionable hint to **stderr**:

```
💡 API not enabled for your GCP project.
   Enable it at: https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=549352339482
   After enabling, wait a few seconds and retry your command.
```

**Steps to fix:**

1. Click the `enable_url` link (or copy it from the `enable_url` JSON field).
2. In the GCP Console, click **Enable**.
3. Wait ~10 seconds, then retry your `gws` command.

> [!TIP]
> You can also run `gws auth setup` which walks you through enabling all required
> APIs for your project automatically.

## Development

```bash
cargo build                       # dev build
cargo clippy -- -D warnings       # lint
cargo test                        # unit tests
./scripts/coverage.sh             # HTML coverage report → target/llvm-cov/html/
```

## License

Apache-2.0

## Disclaimer

> [!CAUTION]
> This is **not** an officially supported Google product.

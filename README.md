<p align="center">
  <img src="https://raw.githubusercontent.com/googleworkspace/cli/refs/heads/main/docs/logo.jpg" alt="gws logo" width="100%">
</p>

<h1 align="center">gws</h1>

<p align="center">
  <strong>One CLI for all of Google Workspace — built for humans and AI agents.</strong><br>
  Drive, Gmail, Calendar, and every Workspace API. Zero boilerplate. Structured JSON output. 40+ agent skills included.
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/@googleworkspace/cli"><img src="https://img.shields.io/npm/v/@googleworkspace/cli" alt="npm version"></a>
  <a href="https://github.com/googleworkspace/cli/blob/main/LICENSE"><img src="https://img.shields.io/github/license/googleworkspace/cli" alt="license"></a>
  <a href="https://github.com/googleworkspace/cli/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/googleworkspace/cli/ci.yml?branch=main&label=CI" alt="CI status"></a>
  <a href="https://www.npmjs.com/package/@googleworkspace/cli"><img src="https://img.shields.io/npm/unpacked-size/@googleworkspace/cli" alt="install size"></a>
</p>

---

```bash
npm install -g @googleworkspace/cli
```

---

`gws` doesn't ship a static list of commands. It reads Google's own [Discovery Service](https://developers.google.com/discovery) at runtime and builds its entire command surface dynamically. When Google adds an API endpoint, `gws` picks it up automatically.

> [!IMPORTANT]
> This project is under active development. Expect breaking changes as we march toward v1.0.

<p align="center">
  <img src="https://raw.githubusercontent.com/googleworkspace/cli/refs/heads/main/docs/demo.gif" alt="Demo">
</p>

## Contents

- [Quick Start](#quick-start)
- [Why gws?](#why-gws)
- [Authentication](#authentication)
- [AI Agent Skills](#ai-agent-skills)
- [Advanced Usage](#advanced-usage)
- [Architecture](#architecture)
- [Development](#development)

## Quick Start

```bash
npm install -g @googleworkspace/cli

gws setup          # walks you through Google Cloud project config + OAuth login
gws drive files list --params '{"pageSize": 5}'
```

Or build from source:

```bash
cargo install --path .
```

---

## Why gws?

**For humans** — stop writing `curl` calls against REST docs. `gws` gives you tab‑completion, `--help` on every resource, `--dry-run` to preview requests, and auto‑pagination.

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

---

## Authentication

The CLI supports multiple auth workflows so it works on your laptop, in CI, and on a server.

### Interactive (local desktop)

Credentials are encrypted at rest (AES-256-GCM) with the key stored in your OS keyring.

```bash
gws setup            # one-time: creates a Cloud project, enables APIs, logs you in
gws auth login       # subsequent logins
```

> Requires the [`gcloud` CLI](https://cloud.google.com/sdk/docs/install) to be installed and authenticated.

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

For Domain-Wide Delegation, add:

```bash
export GOOGLE_WORKSPACE_CLI_IMPERSONATED_USER=admin@example.com
```

### Pre-obtained Access Token

Useful when another tool (e.g. `gcloud`) already mints tokens for your environment.

```bash
export GOOGLE_WORKSPACE_CLI_TOKEN=$(gcloud auth print-access-token)
```

### Precedence

| Priority | Source | Set via |
|----------|--------|---------|
| 1 | Access token | `GOOGLE_WORKSPACE_CLI_TOKEN` |
| 2 | Credentials file | `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` |
| 3 | Encrypted credentials (OS keyring) | `gws auth login` |
| 4 | Plaintext credentials | `~/.config/gws/credentials.json` |

Environment variables can also live in a `.env` file.

---

## AI Agent Skills

The repo ships 40+ [Agent Skills](https://github.com/vercel-labs/agent-skills) (`SKILL.md` files) — one for every supported API, plus higher-level helpers for common workflows like sending email, triaging a Gmail inbox, or subscribing to calendar events.

```bash
# Install all skills at once
npx skills add github:googleworkspace/cli

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

---

## Advanced Usage

### Multipart Uploads

```bash
gws drive files create --json '{"name": "report.pdf"}' --upload ./report.pdf
```

### Pagination

| Flag | Description | Default |
|------|-------------|---------|
| `--page-all` | Auto-paginate, one JSON line per page (NDJSON) | off |
| `--page-limit <N>` | Max pages to fetch | 10 |
| `--page-delay <MS>` | Delay between pages | 100 ms |

### Model Armor (Response Sanitization)

Integrate [Google Cloud Model Armor](https://cloud.google.com/model-armor) to scan API responses for prompt injection before they reach your agent.

```bash
gws gmail users messages get --params '...' \
  --sanitize "projects/P/locations/L/templates/T"
```

| Variable | Description |
|----------|-------------|
| `GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE` | Default Model Armor template |
| `GOOGLE_WORKSPACE_CLI_SANITIZE_MODE` | `warn` (default) or `block` |

---

## Architecture

`gws` uses a **two-phase parsing** strategy:

1. Read `argv[1]` to identify the service (e.g. `drive`)
2. Fetch the service's Discovery Document (cached 24 h)
3. Build a `clap::Command` tree from the document's resources and methods
4. Re-parse the remaining arguments
5. Authenticate, build the HTTP request, execute

All output — success, errors, download metadata — is structured JSON.

---

## Development

```bash
cargo build                       # dev build
cargo clippy -- -D warnings       # lint
cargo test                        # unit tests
./scripts/coverage.sh             # HTML coverage report → target/llvm-cov/html/
```

---

## License

Apache-2.0

## Disclaimer

This is not an officially supported Google product.

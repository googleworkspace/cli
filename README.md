# gws — Google Workspace CLI

A CLI that generates its entire command surface dynamically from Google Discovery Service JSON documents. Includes skills for AI agents.

![Demo](docs/demo.gif)

## Install

```bash
npm install -g @googleworkspace/cli
```

Or build from source:

```bash
cargo install --path .
```

## AI Agents & Skills

This repository includes [Agent Skills](https://github.com/vercel-labs/agent-skills) definitions (`SKILL.md`) for every supported Google Workspace API. Skills are prefixed with `gws-` to avoid namespace collisions when installed globally.

You can install these skills directly into your AI agent using `npx`:

```bash
# Add all Google Workspace skills to your agent
npx skills add github:googleworkspace/cli
```

Or add specific skills by path:

```bash
# Add the shared skill (authentication, etc.)
npx skills add https://github.com/googleworkspace/cli/tree/main/skills/gws-shared

# Add only Google Drive and Gmail skills
npx skills add https://github.com/googleworkspace/cli/tree/main/skills/gws-drive
npx skills add https://github.com/googleworkspace/cli/tree/main/skills/gws-gmail
```

### OpenClaw

Clone the repo and copy (or symlink) the skills into your OpenClaw skills directory:

```bash
# All skills
cp -r skills/gws-* ~/.openclaw/skills/

# Or symlink for easy updates
ln -s $(pwd)/skills/gws-* ~/.openclaw/skills/
```

Or copy only specific skills:

```bash
cp -r skills/gws-drive skills/gws-gmail ~/.openclaw/skills/
```

The `gws-shared` skill includes an `install` block so OpenClaw can auto-install the CLI via `npm i -g @googleworkspace/cli` if the `gws` binary isn't found on PATH.

## Usage

```bash
# List files in Drive
gws drive files list --params '{"pageSize": 10}'

# Get a file's metadata
gws drive files get --params '{"fileId": "abc123"}'

# Create a spreadsheet
gws sheets spreadsheets create --json '{"properties": {"title": "My Sheet"}}'

# List Gmail messages
gws gmail users messages list --params '{"userId": "me"}'

# Introspect a method's schema
gws schema drive.files.list

# Dynamic help for any resource
gws drive files --help
gws drive files list --help

# Preview a request without sending it
gws chat spaces messages create \
  --params '{"parent": "spaces/xyz"}' \
  --json '{"text": "Hello world"}' \
  --dry-run
```

## Authentication

The CLI supports three primary authentication workflows depending on your environment.

### 1. Interactive Auth (Local Desktop)

For interactive use on your personal machine where a web browser is available. 

**Security**: By default, credentials and access tokens are encrypted at rest using AES-256-GCM. The encryption key is stored securely in your OS Keyring (Apple Keychain, Secret Service, or Windows Credential Manager). If a keyring is unavailable (e.g., headless Linux), it falls back to a strictly permissioned (`0600`) local key file.

**Google Cloud Setup & Login:**
The CLI includes a built-in setup wizard to help you configure your Google Cloud Project, enable APIs, and generate the necessary OAuth credentials. Note that this requires the [`gcloud` CLI](https://cloud.google.com/sdk/docs/install) to be installed and authenticated (`gcloud auth login`).

```bash
# Run the interactive setup and login wizard
gws setup

# Or login directly if you already have client_secret.json configured
gws auth login

# Or login with custom scopes
gws auth login --scopes "https://www.googleapis.com/auth/drive,https://www.googleapis.com/auth/gmail.readonly"
```

### 2. Headless & CI/CD Auth (Export Flow)

For remote servers, SSH sessions, or CI/CD pipelines where a browser is unavailable, use the export flow. 

1. On your **local machine** (with a browser), complete the Interactive Auth steps above.
2. Export your credentials to a portable JSON format:
   ```bash
   gws auth export --unmasked > credentials.json
   ```
3. On your **headless machine**, securely transfer `credentials.json` and point the CLI to it. The CLI will automatically use this payload to mint fresh access tokens.
   ```bash
   export GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE=/path/to/credentials.json
   
   # Commands now work headlessly!
   gws drive files list
   ```

*Note: You can also strictly provide a short-lived access token directly via environment variable (e.g. `export GOOGLE_WORKSPACE_CLI_TOKEN=$(gcloud auth print-access-token)`), though this token will naturally expire in ~1 hour.*

### 3. Service Account Auth (Server-to-Server)

For automated programmatic access. Point `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` to your service account JSON key file. No login step is required.

```bash
export GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE=/path/to/service-account.json
gws drive files list
```

**Domain-Wide Delegation (Impersonation)**
If your service account has Domain-Wide Delegation enabled, you can impersonate a Workspace user (e.g., an admin) to perform actions on their behalf.

```bash
export GOOGLE_WORKSPACE_CLI_IMPERSONATED_USER=user@example.com
```

### 4. Pre-obtained Access Token (CI/CD or External)

The simplest way to authenticate if you already possess a short-lived access token. This is often used in CI/CD pipelines where another tool (like `gcloud`) mints the token for the environment.

```bash
# Obtain a token using the gcloud CLI
export GOOGLE_WORKSPACE_CLI_TOKEN=$(gcloud auth print-access-token)
gws drive files list
```
*(Note: These raw access tokens typically expire in ~1 hour).*

---

### Auth Precedence Order

The CLI evaluates authentication sources in the following strict order:

| Priority | Source | How to set |
|----------|--------|------------|
| 1 (highest) | Raw access token | `GOOGLE_WORKSPACE_CLI_TOKEN` env var |
| 2 | Credentials file (user or service account) | `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` env var |
| 3 | Encrypted credentials & token cache | `~/.config/gws/credentials.enc` and `token_cache.json` (created by `gws auth login`, secured via OS Keyring) |
| 4 | Plaintext credentials | `~/.config/gws/credentials.json` |
| — | No auth | Proceeds unauthenticated; shows error if the API rejects |

*(Note: Environment variables can also be set via a `.env` file in the working directory.)*

## Architecture

The CLI uses a **two-phase argument parsing** strategy:

1. Extract the service name from `argv[1]`
2. Fetch the service's Discovery Document (cached for 24h)
3. Build a dynamic `clap::Command` tree from the document's resources/methods
4. Re-parse the remaining arguments against the tree
5. Authenticate, construct the HTTP request, and execute

All output (success, error, file download metadata) is structured JSON for AI agent consumption. Binary outputs require an `--output` flag.

There are a few special behaviors to be aware of that diverge from the Discovery Service API representation:

### Multipart uploads

For multipart uploads (e.g. Drive file uploads), use the `--upload` flag to specify the path to the file to upload.

```bash
gws drive files create --json '{"name": "My File"}' --upload /path/to/file
```

### Pagination and NDJSON

Use `--page-all` to auto-paginate through results. Each page is emitted as a single JSON line (NDJSON), making it easy to stream into tools like `jq`.

| Flag | Description | Default |
| --- | --- | --- |
| `--page-all` | Auto-paginate, one JSON line per page | off |
| `--page-limit <N>` | Max pages to fetch | 10 |
| `--page-delay <MS>` | Delay between pages in ms | 100 |

```bash
# Stream all Drive files as NDJSON
gws drive files list --params '{"pageSize": 100}' --page-all --page-limit 5

# Pipe to jq to extract file names
gws drive files list --params '{"pageSize": 100}' --page-all | jq -r '.files[].name'
```

## Testing & Coverage

Run unit tests:
```bash
cargo test
```

Generate code coverage report (requires `cargo-llvm-cov`):
```bash
./scripts/coverage.sh
```
The report will be available at `target/llvm-cov/html/index.html`.

## Security & Sanitization (Model Armor)
 
 The CLI integrates with **Google Cloud Model Armor** to sanitize API responses for prompt injection risks before they reach your AI agent.
 
 ```bash
 # Sanitize a specific command
 gws gmail users messages get --params '...' \
   --sanitize "projects/P/locations/L/templates/T"
 ```
 
 This checks the *entire* JSON response against the specified Model Armor template.
 
 ### Configuration
 
 You can set default behavior via environment variables:
 
 | Variable | Description |
 |---|---|
 | `GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE` | Default Model Armor template resource name |
 | `GOOGLE_WORKSPACE_CLI_SANITIZE_MODE` | `warn` (default) or `block`. |
 
 - **Warn mode**: Prints a warning to stderr and annotates the JSON with `_sanitization` details.
 - **Block mode**: Suppresses the output entirely and exits with an error if a match is found.
 
 ### Requirements
 
 Using `--sanitize` requires the `https://www.googleapis.com/auth/cloud-platform` scope.
 
 ## License

Apache-2.0

## Disclaimer

This is not an officially supported Google product.

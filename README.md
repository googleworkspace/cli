# gws — Google Workspace CLI

A CLI that generates its entire command surface dynamically from Google Discovery Service JSON documents.

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
```

## Authentication

### Auth Precedence Order

The CLI tries authentication sources in the following order:

| Priority | Source | How to set |
|----------|--------|------------|
| 1 (highest) | Raw access token | `GOOGLE_WORKSPACE_CLI_TOKEN` env var |
| 2 | Credentials file (user or service account) | `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` env var |
| 3 | Encrypted credentials | `~/.config/gws/credentials.enc` (created by `gws auth login`) |
| 4 | Plaintext credentials | `~/.config/gws/credentials.json` |
| — | No auth | Proceeds unauthenticated; shows error if the API rejects |

Environment variables can also be set via a `.env` file in the working directory.

#### Using a pre-obtained token

The simplest way to authenticate — useful in CI/CD or when a token is obtained externally:

```bash
export GOOGLE_WORKSPACE_CLI_TOKEN=$(gcloud auth print-access-token)
gws drive files list --params '{"pageSize": 10}'
```

### Managing Auth with the CLI

**Credential storage:** When you run `gws auth login`, credentials are encrypted at rest using AES-256-GCM with a key derived from your hostname and username. This means the encrypted file (`~/.config/gws/credentials.enc`) can't be used on a different machine. Plaintext credentials via `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` are also supported.

```bash
# Login — opens browser for OAuth2 consent
# Requires GOOGLE_WORKSPACE_CLI_CLIENT_ID and GOOGLE_WORKSPACE_CLI_CLIENT_SECRET
gws auth login

# Login with read-only scopes
gws auth login --readonly

# Login with custom scopes
gws auth login --scopes "https://www.googleapis.com/auth/drive,https://www.googleapis.com/auth/gmail.readonly"

# Check current auth state
gws auth status

# Logout — clears saved credentials and token cache
gws auth logout
```

Set your OAuth client credentials in `.env` or as environment variables:

```bash
GOOGLE_WORKSPACE_CLI_CLIENT_ID=your_client_id.apps.googleusercontent.com
GOOGLE_WORKSPACE_CLI_CLIENT_SECRET=your_client_secret
```

Create an OAuth client ID at [Google Cloud Console → Credentials](https://console.cloud.google.com/apis/credentials). Choose **Desktop app** as the application type.

### Google Cloud Setup

```bash
# 1. Create a project (or use an existing one)
gcloud projects create my-gws-cli --name="GWS CLI"
gcloud config set project my-gws-cli

# 2. Enable all Google Workspace APIs
gcloud services enable --project my-gws-cli \
  drive.googleapis.com \
  sheets.googleapis.com \
  gmail.googleapis.com \
  calendar-json.googleapis.com \
  docs.googleapis.com \
  slides.googleapis.com \
  tasks.googleapis.com \
  people.googleapis.com \
  chat.googleapis.com \
  vault.googleapis.com \
  groupssettings.googleapis.com \
  reseller.googleapis.com \
  licensing.googleapis.com \
  script.googleapis.com \
  admin.googleapis.com \
  classroom.googleapis.com \
  cloudidentity.googleapis.com \
  alertcenter.googleapis.com \
  forms.googleapis.com \
  keep.googleapis.com \
  meet.googleapis.com

# 3. Create an OAuth consent screen (choose "External" or "Internal")
#    Visit: https://console.cloud.google.com/apis/credentials/consent

# 4. Create an OAuth Desktop client ID
#    Visit: https://console.cloud.google.com/apis/credentials
#    Choose "Desktop app" as the application type

# 5. Set your client credentials
export GOOGLE_WORKSPACE_CLI_CLIENT_ID=your_client_id.apps.googleusercontent.com
export GOOGLE_WORKSPACE_CLI_CLIENT_SECRET=your_client_secret

# 6. Login
gws auth login
```



### Service Account & Domain-Wide Delegation

To use a Service Account, point `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` to your service account JSON key file. No login step is required.

```bash
export GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE=/path/to/service-account.json
# Commands will authenticate using the service account key
gws drive files list
```

**Domain-Wide Delegation (Impersonation)**

If your service account has Domain-Wide Delegation enabled, you can impersonate a user (e.g., an admin) to perform actions on their behalf.

```bash
export GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE=/path/to/service-account.json
export GOOGLE_WORKSPACE_CLI_IMPERSONATED_USER=admin@example.com

# Now commands will run as admin@example.com
gws users list --domain example.com
```

## Supported Services


| Command              | API                   | Version      | Aliases     |
| -------------------- | --------------------- | ------------ | ----------- |
| `gws drive`          | Drive                 | v3           |             |
| `gws sheets`         | Sheets                | v4           |             |
| `gws gmail`          | Gmail                 | v1           |             |
| `gws calendar`       | Calendar              | v3           |             |
| `gws admin`          | Admin SDK (Directory) | directory_v1 | `directory` |
| `gws admin-reports`  | Admin SDK (Reports)   | reports_v1   | `reports`   |
| `gws docs`           | Docs                  | v1           |             |
| `gws slides`         | Slides                | v1           |             |
| `gws tasks`          | Tasks                 | v1           |             |
| `gws people`         | People                | v1           |             |
| `gws chat`           | Chat                  | v1           |             |
| `gws vault`          | Vault                 | v1           |             |
| `gws groupssettings` | Groups Settings       | v1           |             |
| `gws reseller`       | Reseller              | v1           |             |
| `gws licensing`      | Licensing             | v1           |             |
| `gws apps-script`    | Apps Script           | v1           | `script`    |
| `gws classroom`      | Classroom             | v1           |             |
| `gws cloudidentity`  | Cloud Identity        | v1           |             |
| `gws alertcenter`    | Alert Center          | v1beta1      |             |
| `gws forms`          | Forms                 | v1           |             |
| `gws keep`           | Keep                  | v1           |             |
| `gws meet`           | Meet                  | v2           |             |

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

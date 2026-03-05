---
name: gws
description: "Google Workspace CLI: Interact with Drive, Gmail, Calendar, Sheets, and all Workspace APIs using the gws command-line tool. Use when managing Google Workspace resources, sending emails, listing files, creating calendar events, or querying any Google API."
---

# gws — Google Workspace CLI

`gws` provides dynamic access to every Google Workspace API (Drive, Gmail, Calendar, Sheets, Admin, and more) by parsing Google Discovery Documents at runtime. No generated crates or static API bindings are needed — when Google adds an endpoint, `gws` picks it up automatically.

## Installation

```bash
npm install -g @googleworkspace/cli
```

The `gws` binary must be on `$PATH`.

## Authentication

```bash
gws auth setup     # one-time: creates a Cloud project, enables APIs, logs in
gws auth login     # subsequent logins with scope selection
```

For headless/CI:

```bash
export GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE=/path/to/credentials.json
# or
export GOOGLE_WORKSPACE_CLI_TOKEN=$(gcloud auth print-access-token)
```

## Core Syntax

```bash
gws <service> <resource> [sub-resource] <method> [flags]
```

Use `--help` at any level to discover available commands:

```bash
gws --help
gws <service> --help
gws <service> <resource> --help
gws <service> <resource> <method> --help
```

## Key Flags

| Flag | Description |
|------|-------------|
| `--params '<JSON>'` | URL/query parameters |
| `--json '<JSON>'` | Request body for POST/PUT/PATCH |
| `--fields '<MASK>'` | Limit response fields (critical for context efficiency) |
| `--page-all` | Auto-paginate results as NDJSON |
| `--page-limit <N>` | Max pages when using --page-all (default: 10) |
| `--upload <PATH>` | Upload file content (multipart) |
| `--output <PATH>` | Save binary responses to file |
| `--dry-run` | Validate locally without calling the API |
| `--sanitize <TEMPLATE>` | Screen responses through Model Armor |

## Schema Introspection

Before calling any API method, inspect its parameters and request body:

```bash
gws schema drive.files.list
gws schema sheets.spreadsheets.create
```

## Common Patterns

### Reading data (always use --fields to minimize output)

```bash
gws drive files list --params '{"pageSize": 10}' --fields "files(id,name,mimeType)"
gws gmail users messages get --params '{"userId": "me", "id": "MSG_ID"}'
gws calendar events list --params '{"calendarId": "primary", "maxResults": 5}'
```

### Writing data

```bash
gws sheets spreadsheets create --json '{"properties": {"title": "Q1 Budget"}}'
gws calendar events insert --params '{"calendarId": "primary"}' --json '{"summary": "Team Sync", "start": {"dateTime": "2025-01-15T10:00:00Z"}, "end": {"dateTime": "2025-01-15T11:00:00Z"}}'
```

### Pagination

```bash
gws drive files list --params '{"pageSize": 100}' --page-all | jq -r '.files[].name'
```

### File uploads and downloads

```bash
gws drive files create --json '{"name": "report.pdf"}' --upload ./report.pdf
gws drive files get --params '{"fileId": "FILE_ID", "alt": "media"}' --output ./downloaded.pdf
```

## Security Rules

- Never output secrets (API keys, tokens) directly
- Always confirm with the user before executing write/delete commands
- Prefer `--dry-run` for destructive operations
- Use `--sanitize` for PII/content safety screening

## Available Services

admin, admin-reports, alertcenter, apps-script, calendar, chat, classroom, cloudidentity, docs, drive, events, forms, gmail, groupssettings, keep, licensing, meet, modelarmor, people, reseller, sheets, slides, tasks, vault

For the full list of service skills and recipes, see the `skills/` directory in the repository.

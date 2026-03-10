# gws MCP Server

> MCP (Model Context Protocol) server for Google Workspace — wraps the [`gws`](https://github.com/googleworkspace/cli) CLI as MCP tools.

Any MCP-compatible client (Claude Desktop, Cursor, Zed, Windsurf, etc.) can use this server to interact with Google Workspace APIs: Drive, Gmail, Calendar, Sheets, Docs, Chat, and more.

## Prerequisites

1. **Node.js 18+**
2. **`gws` CLI** installed and authenticated:
   ```bash
   npm install -g @googleworkspace/cli
   gws auth login
   ```

## Quick Start

### Claude Desktop

Add to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "google-workspace": {
      "command": "npx",
      "args": ["-y", "@googleworkspace/mcp-server"]
    }
  }
}
```

### Cursor / Windsurf

Add to your MCP settings:

```json
{
  "google-workspace": {
    "command": "npx",
    "args": ["-y", "@googleworkspace/mcp-server"]
  }
}
```

### Manual

```bash
cd mcp-server
npm install
npm run build
node dist/index.js   # communicates over stdio
```

## Available Tools

### Per-Service Tools

Each Google Workspace service has a dedicated tool:

| Tool | Service |
|------|---------|
| `gws_drive` | Google Drive — files, folders, shared drives |
| `gws_gmail` | Gmail — send, read, manage email |
| `gws_calendar` | Google Calendar — events, calendars |
| `gws_sheets` | Google Sheets — read/write spreadsheets |
| `gws_docs` | Google Docs — read/write documents |
| `gws_slides` | Google Slides — presentations |
| `gws_tasks` | Google Tasks — task lists |
| `gws_people` | Google People — contacts & profiles |
| `gws_chat` | Google Chat — spaces & messages |
| `gws_classroom` | Google Classroom — classes & coursework |
| `gws_forms` | Google Forms — read/write forms |
| `gws_keep` | Google Keep — notes |
| `gws_meet` | Google Meet — conferences |
| `gws_admin_reports` | Admin — audit logs & usage reports |

Each tool accepts:
- `resource` — API resource (e.g. `files`, `messages`)
- `method` — API method (e.g. `list`, `get`, `create`)
- `params` — JSON query/path parameters
- `body` — JSON request body
- `dry_run` — validate without calling API
- `extra_args` — additional CLI flags

### Utility Tools

| Tool | Description |
|------|-------------|
| `gws_run` | Run any arbitrary `gws` command |
| `gws_schema` | Introspect API method schemas |
| `gws_auth_status` | Check authentication status |

### Resources

| Resource | Description |
|----------|-------------|
| `gws://services` | List all available services |

## Examples

Once connected, you can ask your AI assistant:

- *"List my 10 most recent Google Drive files"*
- *"Send an email to team@example.com about the Q1 report"*
- *"What meetings do I have tomorrow?"*
- *"Create a new spreadsheet called Budget 2026"*
- *"Show me unread emails from the last hour"*

The AI will use the appropriate `gws_*` tool with `gws_schema` for discovery.

## Architecture

```
┌─────────────────┐     stdio      ┌──────────────┐     exec     ┌─────┐
│  MCP Client     │ ◄────────────► │  MCP Server  │ ──────────►  │ gws │
│  (Claude, etc.) │                │  (this pkg)  │              │ CLI │
└─────────────────┘                └──────────────┘              └─────┘
                                                                    │
                                                            Google Workspace
                                                               APIs
```

The server wraps the `gws` CLI rather than calling Google APIs directly. This means:
- **Auth is handled by gws** — no extra credential management
- **All gws features work** — pagination, dry-run, Model Armor, etc.
- **Stays in sync** — when gws adds APIs, the `gws_run` tool covers them immediately

## License

Apache-2.0 — see [LICENSE](../LICENSE)

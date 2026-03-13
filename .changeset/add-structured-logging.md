---
"@googleworkspace/cli": minor
---

Add opt-in structured HTTP request logging via `tracing`

New environment variables:
- `GOOGLE_WORKSPACE_CLI_LOG`: stderr log filter (e.g., `gws=debug`)
- `GOOGLE_WORKSPACE_CLI_LOG_FILE`: directory for JSON log files with daily rotation

Logging is completely silent by default (zero overhead). Only PII-free metadata is logged: API method ID, HTTP method, status code, latency, and content-type.

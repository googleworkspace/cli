---
"@googleworkspace/cli": minor
---

Replace `+triage` with `+search` for full-metadata Gmail search with label resolution and pagination.

Breaking: `+triage` is removed. Use `gws gmail +search --query 'is:unread'` instead.
Breaking: `+read --format json` field names change from snake_case to camelCase
(e.g. `thread_id` → `threadId`, `body_text` → `bodyText`) for consistency with `+search`
and the project's camelCase JSON convention.
Other changes: `--query` is now required, default output format is JSON (was table),
output schema includes structured from/to/cc, resolved labels, threadId, and snippet.

---
"@googleworkspace/cli": minor
---

feat(mcp): per-user Google token isolation for HTTP transport (Phase 4). Each authenticated MCP user's tool calls now use their own GWS access token obtained during the OAuth flow. Scopes are derived from the configured service list (`-s gmail -s drive`) at authorize time via `gws_scopes_for_services`. Tokens are refreshed transparently when near-expiry using the stored refresh_token. Falls back to the shared `gws auth login` credential when `--auth` is disabled.

**Current limitations**: per-user tokens are in-memory only (cleared on restart); services not in DEFAULT_SCOPES (e.g. `admin`, `script`) may lack their specific GWS scopes at authorize time.

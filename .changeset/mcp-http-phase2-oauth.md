---
"@googleworkspace/cli": minor
---

feat(mcp): add OAuth2 PKCE Authorization Server for HTTP transport (Phase 2/3). Enable with `gws mcp --transport http --auth`. Implements MCP Authorization spec 2025-11-25: RFC 9728 protected-resource metadata, RFC 8414 AS metadata, DCR stub, PKCE S256 authorization/token flow via Google OAuth2. All `/mcp` requests require `Authorization: Bearer` when `--auth` is active; unauthenticated requests receive 401 with `WWW-Authenticate` pointing to the AS metadata URL. Sessions expire after 8 hours. Requires `client_secret.json` from `gws auth setup`.

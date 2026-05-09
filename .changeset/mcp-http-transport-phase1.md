---
"@googleworkspace/cli": minor
---

feat(mcp): add Streamable HTTP transport (Phase 1, no auth). Start with `gws mcp -s gmail --transport http --port 3000`; Claude Desktop config changes to `{"url": "http://localhost:3000/mcp"}`. Uses rmcp 1.x `transport-streamable-http-server` feature via axum. Stdio transport unchanged.

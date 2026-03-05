---
"@googleworkspace/cli": patch
---

Fix MCP tool schemas to conditionally include `body` and `upload` properties only when the underlying Discovery Document method supports them. Also drops empty `body: {}` objects that LLMs commonly send on GET methods, preventing 400 errors from Google APIs.

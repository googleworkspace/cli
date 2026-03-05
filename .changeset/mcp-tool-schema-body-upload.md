---
"@googleworkspace/cli": patch
---

Fix MCP tool schemas to conditionally include `body`, `upload`, and `page_all` properties only when the underlying Discovery Document method supports them. `body` is included only when a request body is defined, `upload` only when `supportsMediaUpload` is true, and `page_all` only when the method has a `pageToken` parameter. Also drops empty `body: {}` objects that LLMs commonly send on GET methods, preventing 400 errors from Google APIs.

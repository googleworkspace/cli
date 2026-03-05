---
"@googleworkspace/cli": patch
---

Add Content-Length: 0 header for POST/PUT/PATCH requests with no body to fix HTTP 411 errors

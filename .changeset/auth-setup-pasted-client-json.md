---
"@googleworkspace/cli": patch
---

`gws auth setup` accepts a pasted `client_secret.json` document and stores its `client_id` and `client_secret` instead of writing a double-serialized config whose fields held the whole document, which every later login rejected with `401 invalid_client`.

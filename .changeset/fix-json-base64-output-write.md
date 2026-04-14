---
"@googleworkspace/cli": patch
---

Make `--output` decode JSON-wrapped base64url payloads (for example Gmail attachment responses) and write bytes to disk instead of silently succeeding without a file.

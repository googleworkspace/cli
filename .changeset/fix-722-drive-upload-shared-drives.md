---
"@googleworkspace/cli": patch
---

`drive +upload`: pass `supportsAllDrives=true` on every upload request so files can be placed inside Shared Drive folders via `--parent`. Previously, any `--parent` pointing to a Shared Drive folder was rejected by the API with a 404 or permission error even when the user had access.

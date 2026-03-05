---
"@googleworkspace/cli": patch
---

Add `-s`/`--services` flag to `gws auth login` to filter the scope picker
by service name (e.g. `-s drive,gmail,sheets`). Also expands the workspace
admin scope blocklist to include `chat.admin.*` and `classroom.*` patterns.

---
"@googleworkspace/cli": patch
---

fix: add -s / --services flag to `gws auth login` to limit OAuth scope picker

`gws auth login` previously requested scopes for every API enabled in the GCP
project, including Workspace-admin-only APIs (`cloud-identity.*`, `apps.alerts`,
`ediscovery`, etc.) that return `400: invalid_scope` for personal Gmail accounts.

This patch adds a `-s` / `--services` flag that restricts the interactive scope
picker to only the specified services:

```
gws auth login -s drive,gmail,calendar,docs,sheets
```

When `-s` is given:
- With `--project` / saved project ID: only APIs in both the enabled set and the
  `-s` list are shown in the picker.
- Without a project ID: scopes are fetched directly from discovery for the
  specified services, bypassing the "all enabled APIs" expansion entirely.

Unknown service names produce a warning and are silently skipped.

Also adds `api_id_for_service()` helper to `setup.rs` (maps CLI service names
such as `"drive"` to their GCP service IDs like `"drive.googleapis.com"`).

Fixes #138

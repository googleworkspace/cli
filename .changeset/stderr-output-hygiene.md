---
"@googleworkspace/cli": patch
---

Stderr/output hygiene rollup: route diagnostics to stderr, add colored error labels, propagate auth errors.

- **triage.rs**: "No messages found" sent to stderr so stdout stays valid JSON for pipes
- **modelarmor.rs**: response body printed only on success; error message now includes body for diagnostics
- **error.rs**: colored `error[variant]:` labels on stderr (respects `NO_COLOR` env var), `hint:` prefix for accessNotConfigured guidance
- **calendar, chat, docs, drive, script, sheets**: auth failures now propagate as `GwsError::Auth` instead of silently proceeding unauthenticated (dry-run still works without auth)

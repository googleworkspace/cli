---
"@googleworkspace/cli": minor
---

feat(auth): re-add service account impersonation via `--subject` flag and `GOOGLE_WORKSPACE_CLI_SUBJECT` env var for domain-wide delegation (DWD).

When a service account has domain-wide delegation enabled, executive assistants can now use it to operate on a executive's Google Workspace account:

```bash
# Via CLI flag
gws --subject boss@company.com gmail +triage --max 10

# Via env var
export GOOGLE_WORKSPACE_CLI_SUBJECT=boss@company.com
gws gmail +triage --max 10
```

This restores the functionality that was removed in #250 and #253, with a minimal surface area — no multi-account registry, no `accounts.json`.

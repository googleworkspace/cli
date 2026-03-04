---
name: gws-alertcenter
version: 1.0.0
description: "Google Workspace Alert Center: Manage Workspace security alerts."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws alertcenter --help"
---

# alertcenter (v1beta1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws alertcenter <resource> <method> [flags]
```

## API Resources

### alerts

  - `batchDelete` — Performs batch delete operation on alerts.
  - `batchUndelete` — Performs batch undelete operation on alerts.
  - `delete` — Marks the specified alert for deletion. An alert that has been marked for deletion is removed from Alert Center after 30 days. Marking an alert for deletion has no effect on an alert which has already been marked for deletion. Attempting to mark a nonexistent alert for deletion results in a `NOT_FOUND` error.
  - `get` — Gets the specified alert. Attempting to get a nonexistent alert returns `NOT_FOUND` error.
  - `getMetadata` — Returns the metadata of an alert. Attempting to get metadata for a non-existent alert returns `NOT_FOUND` error.
  - `list` — Lists the alerts.
  - `undelete` — Restores, or "undeletes", an alert that was marked for deletion within the past 30 days. Attempting to undelete an alert which was marked for deletion over 30 days ago (which has been removed from the Alert Center database) or a nonexistent alert returns a `NOT_FOUND` error. Attempting to undelete an alert which has not been marked for deletion has no effect.
  - `feedback` — Operations on the 'feedback' resource

### v1beta1

  - `getSettings` — Returns customer-level settings.
  - `updateSettings` — Updates the customer-level settings.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws alertcenter --help

# Inspect a method's required params, types, and defaults
gws schema alertcenter.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


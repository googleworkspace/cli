---
name: gws-groupssettings
version: 1.0.0
description: "USE WHEN the user wants to manage google groups settings via the `gws` CLI."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws groupssettings --help"
---

# groupssettings (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws groupssettings <resource> <method> [flags]
```

## API Resources

### groups

  - `get` — Gets one resource by id.
  - `patch` — Updates an existing resource. This method supports patch semantics.
  - `update` — Updates an existing resource.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws groupssettings --help

# Inspect a method's required params, types, and defaults
gws schema groupssettings.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


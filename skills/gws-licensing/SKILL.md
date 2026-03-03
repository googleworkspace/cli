---
name: gws-licensing
version: 1.0.0
description: "Google Workspace Enterprise License Manager: Manage product licenses."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws licensing --help"
---

# licensing (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws licensing <resource> <method> [flags]
```

## API Resources

### licenseAssignments

  - `delete` — Revoke a license.
  - `get` — Get a specific user's license by product SKU.
  - `insert` — Assign a license.
  - `listForProduct` — List all users assigned licenses for a specific product SKU.
  - `listForProductAndSku` — List all users assigned licenses for a specific product SKU.
  - `patch` — Reassign a user's product SKU with a different SKU in the same product. This method supports patch semantics.
  - `update` — Reassign a user's product SKU with a different SKU in the same product.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws licensing --help

# Inspect a method's required params, types, and defaults
gws schema licensing.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


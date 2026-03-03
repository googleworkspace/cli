---
name: recipe-audit-external-sharing
version: 1.0.0
description: "Find and review Google Drive files shared outside the organization."
metadata:
  openclaw:
    category: "recipe"
    domain: "security"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Audit External Drive Sharing

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

Find and review Google Drive files shared outside the organization.

> [!CAUTION]
> Revoking permissions immediately removes access. Confirm with the file owner first.

## Steps

1. List externally shared files: `gws drive files list --params '{"q": "visibility = '\''anyoneWithLink'\''"}'`
2. Check permissions on a file: `gws drive permissions list --params '{"fileId": "FILE_ID"}'`
3. Revoke if needed: `gws drive permissions delete --params '{"fileId": "FILE_ID", "permissionId": "PERM_ID"}'`


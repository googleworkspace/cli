---
name: recipe-audit-drive-sharing
version: 1.0.0
description: "USE WHEN the user needs to check which files are shared externally."
metadata:
  openclaw:
    category: "recipe"
    domain: "security"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Audit External Sharing on Drive

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

USE WHEN the user needs to check which files are shared externally.

> [!CAUTION]
> Revoking permissions immediately removes access. Confirm with the file owner first.

## Steps

1. Search for externally shared files: `gws drive files list --params '{"q": "visibility = '\''anyoneWithLink'\'' or visibility = '\''anyoneCanFind'\''"}'`
2. Check permissions on a specific file: `gws drive permissions list --params '{"fileId": "FILE_ID"}'`
3. Revoke external access if needed: `gws drive permissions delete --params '{"fileId": "FILE_ID", "permissionId": "PERM_ID"}'`


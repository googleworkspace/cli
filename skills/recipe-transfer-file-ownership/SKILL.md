---
name: recipe-transfer-file-ownership
version: 1.0.0
description: "Transfer ownership of Google Drive files from one user to another."
metadata:
  openclaw:
    category: "recipe"
    domain: "it"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Transfer File Ownership

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

Transfer ownership of Google Drive files from one user to another.

> [!CAUTION]
> Transferring ownership is irreversible without the new owner's cooperation.

## Steps

1. List files owned by the user: `gws drive files list --params '{"q": "'\''user@company.com'\'' in owners"}'`
2. Transfer ownership: `gws drive permissions create --params '{"fileId": "FILE_ID", "transferOwnership": true}' --json '{"role": "owner", "type": "user", "emailAddress": "newowner@company.com"}'`


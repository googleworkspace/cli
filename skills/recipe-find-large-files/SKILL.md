---
name: recipe-find-large-files
version: 1.0.0
description: "Identify large Google Drive files consuming storage quota."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Find Largest Files in Drive

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

Identify large Google Drive files consuming storage quota.

> [!CAUTION]
> Deleting files is permanent if the trash is emptied. Confirm before deleting.

## Steps

1. List files sorted by size: `gws drive files list --params '{"orderBy": "quotaBytesUsed desc", "pageSize": 20, "fields": "files(id,name,size,mimeType,owners)"}' --format table`
2. Review the output and identify files to delete or move
3. Delete if needed: `gws drive files delete --params '{"fileId": "FILE_ID"}'`


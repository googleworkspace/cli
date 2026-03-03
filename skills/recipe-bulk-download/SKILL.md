---
name: recipe-bulk-download
version: 1.0.0
description: "USE WHEN the user needs to download multiple files from a Drive folder."
metadata:
  openclaw:
    category: "recipe"
    domain: "documents"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Download Multiple Drive Files

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

USE WHEN the user needs to download multiple files from a Drive folder.

## Steps

1. List files in the folder: `gws drive files list --params '{"q": "'\''FOLDER_ID'\'' in parents"}' --format json`
2. Download each file: `gws drive files get --params '{"fileId": "FILE_ID", "alt": "media"}' -o filename.ext`
3. For Google Docs format, use export: `gws drive files export --params '{"fileId": "FILE_ID", "mimeType": "application/pdf"}' -o document.pdf`


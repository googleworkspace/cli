---
name: recipe-batch-rename-files
version: 1.0.0
description: "Rename multiple Google Drive files matching a pattern to follow a consistent naming convention."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Batch Rename Google Drive Files

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

Rename multiple Google Drive files matching a pattern to follow a consistent naming convention.

## Steps

1. Find files to rename: `gws drive files list --params '{"q": "name contains '\''Report'\''"}' --format table`
2. Rename a file: `gws drive files update --params '{"fileId": "FILE_ID"}' --json '{"name": "2025-Q1 Report - Final"}'`
3. Verify the rename: `gws drive files get --params '{"fileId": "FILE_ID", "fields": "name"}'`


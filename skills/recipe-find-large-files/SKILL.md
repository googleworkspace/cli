---
name: recipe-find-large-files
version: 1.0.0
description: "Finds and lists the largest files in Google Drive consuming storage quota, sorted by size. Use when a user wants to free up Google Drive space, their Drive storage is full or nearly full, they need to find big files in Drive, clean up Drive storage, identify what's taking up space, or address a storage quota exceeded warning. Capabilities include listing files by size (largest first), showing file name, type, size, and owner, and identifying candidates to archive, move, or delete."
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

## Steps

1. List files sorted by size: `gws drive files list --params '{"orderBy": "quotaBytesUsed desc", "pageSize": 20, "fields": "files(id,name,size,mimeType,owners)"}' --format table`

   The output table will include columns for **id**, **name**, **size**, **mimeType**, and **owners**, making it easy to spot large files and who owns them. Example output:

   | id            | name             | size       | mimeType                  | owners        |
   |---------------|------------------|------------|---------------------------|---------------|
   | 1aBcDeFgHiJ…  | backup-2023.zip  | 2147483648 | application/zip           | user@example.com |
   | 2kLmNoPqRsT…  | recording.mp4    | 1073741824 | video/mp4                 | user@example.com |
   | 3uVwXyZaAbB…  | report.pdf       | 524288000  | application/pdf           | colleague@example.com |

2. Before acting on any file, verify it is safe to remove:
   - Check the **last modified date** to avoid deleting recently active files (use the `gws-drive` skill to retrieve file metadata if needed)
   - Confirm file **contents or purpose** if the name is ambiguous before treating it as a deletion candidate
   - Check `mimeType` to distinguish Google-native files (which may not count against quota) from uploaded binaries (which do)
   - Check `owners` to confirm you have permission to act on each file

3. Review the verified candidates and take action using the `gws-drive` skill:
   - Note files with the largest `size` values as primary candidates for cleanup
   - Delete, move, or change sharing settings on confirmed candidates

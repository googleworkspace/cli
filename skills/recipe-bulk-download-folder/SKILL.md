---
name: recipe-bulk-download-folder
version: 1.0.0
description: "Lists and downloads all files from a Google Drive folder using the gws CLI. Use when a user wants to bulk download, fetch, or save files from a Google Drive or GDrive folder, access Drive files locally, download from a shared folder, or retrieve cloud files from a specific Drive directory. Handles native files by exporting as PDF."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Bulk Download Drive Folder

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

List and download all files from a Google Drive folder.

## Steps

1. List files in folder: `gws drive files list --params '{"q": "'\''FOLDER_ID'\'' in parents"}' --format json`
2. Download each file: `gws drive files get --params '{"fileId": "FILE_ID", "alt": "media"}' -o filename.ext`
3. Export Google Docs as PDF: `gws drive files export --params '{"fileId": "FILE_ID", "mimeType": "application/pdf"}' -o document.pdf`
4. Handle pagination if the folder contains many files: re-run the list command with `--params '{"q": "'\''FOLDER_ID'\'' in parents", "pageToken": "NEXT_PAGE_TOKEN"}'` using the `nextPageToken` value from the previous response, repeating until no `nextPageToken` is returned.
5. Verify downloads: run `ls -la | wc -l` to count the downloaded files and confirm the total matches the number of files returned by the list command; also run `ls -lah` to inspect file sizes and ensure no files are unexpectedly empty (0 bytes).

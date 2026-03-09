---
name: recipe-organize-drive-folder
version: 1.0.0
description: "Organizes files into a Google Drive folder structure by creating nested folders and moving files into the correct locations. Use when a user wants to organize Google Drive, create folders or directory structures in Drive, sort or move files into Drive folders, manage file organization, or set up a Google Drive folder hierarchy. Supports nested sub-folder creation, bulk file moves, and structure verification."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Organize Files into Google Drive Folders

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

Create a Google Drive folder structure and move files into the right locations.

## Steps

1. Create a project folder: `gws drive files create --json '{"name": "Q2 Project", "mimeType": "application/vnd.google-apps.folder"}'`
   - Capture the `id` field from the JSON response — this is your `PARENT_FOLDER_ID` for subsequent steps.
   - Verify success: `gws drive files get --params '{"fileId": "PARENT_FOLDER_ID"}' --format table`

2. Create sub-folders: `gws drive files create --json '{"name": "Documents", "mimeType": "application/vnd.google-apps.folder", "parents": ["PARENT_FOLDER_ID"]}'`
   - Capture the `id` field from the response as the sub-folder ID.
   - Verify the sub-folder appears under the parent: `gws drive files list --params '{"q": "PARENT_FOLDER_ID in parents"}' --format table`

3. Move existing files into folder: `gws drive files update --params '{"fileId": "FILE_ID", "addParents": "FOLDER_ID", "removeParents": "OLD_PARENT_ID"}'`
   - If the command returns a permission error, confirm the authenticated account has edit access to the file.
   - If `FILE_ID` is not found, use `gws drive files list` to locate the correct ID before retrying.

4. Verify final structure: `gws drive files list --params '{"q": "FOLDER_ID in parents"}' --format table`

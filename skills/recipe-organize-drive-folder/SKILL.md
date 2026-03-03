---
name: recipe-organize-drive-folder
version: 1.0.0
description: "USE WHEN the user needs to create a folder structure in Drive."
metadata:
  openclaw:
    category: "recipe"
    domain: "documents"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Create and Organize a Drive Folder

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

USE WHEN the user needs to create a folder structure in Drive.

## Steps

1. Create the parent folder: `gws drive files create --json '{"name": "Project Folder", "mimeType": "application/vnd.google-apps.folder"}'`
2. Create a subfolder: `gws drive files create --json '{"name": "Documents", "mimeType": "application/vnd.google-apps.folder", "parents": ["PARENT_FOLDER_ID"]}'`
3. Upload files to the folder: `gws drive +upload myfile.pdf --parent FOLDER_ID`
4. List contents: `gws drive files list --params '{"q": "'\''FOLDER_ID'\'' in parents"}'`


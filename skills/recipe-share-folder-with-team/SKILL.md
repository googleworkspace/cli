---
name: recipe-share-folder-with-team
version: 1.0.0
description: "Shares a Google Drive folder and all its contents with a list of collaborators, setting permission levels (editor/viewer) for each person. Use when a user wants to share a Google Drive folder with their team, give access to a folder, add collaborators, invite team members, grant access to files, manage Drive permissions, set sharing settings, or use phrases like 'share with team', 'invite to folder', 'give editor/viewer access', or 'bulk share Drive folder'."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Share a Google Drive Folder with a Team

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

Share a Google Drive folder and all its contents with a list of collaborators, assigning each person an appropriate permission level (editor or viewer).

## Steps

1. Find the folder: `gws drive files list --params '{"q": "name = '\''Project X'\'' and mimeType = '\''application/vnd.google-apps.folder'\''"}'`
2. **Validate the result:** Confirm the search returned exactly one folder. If multiple folders are returned, narrow the query (e.g. add a `parents` filter or check the `id` and `name` fields to identify the correct one). If no folder is found, verify the folder name spelling and that the authenticated account has access to it.
3. Share as editor: `gws drive permissions create --params '{"fileId": "FOLDER_ID"}' --json '{"role": "writer", "type": "user", "emailAddress": "colleague@company.com"}'`
4. Share as viewer: `gws drive permissions create --params '{"fileId": "FOLDER_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "stakeholder@company.com"}'`
   - **If permission creation fails:** Check that the email address is valid and belongs to a Google account. A `400` error typically indicates an invalid address; a `403` error indicates the authenticated account lacks sharing rights on the folder.
5. Verify permissions: `gws drive permissions list --params '{"fileId": "FOLDER_ID"}' --format table`

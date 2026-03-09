---
name: recipe-email-drive-link
version: 1.0.0
description: "Shares a Google Drive file and emails the link with a message to one or more recipients. Use when a user wants to share a Google Drive file via email, send a Drive link, share document access, grant view/edit/comment permissions on a Google Doc or Sheet, or email a Google Docs/Sheets/Slides link to someone."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-drive", "gws-gmail"]
---

# Email a Google Drive File Link

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`, `gws-gmail`

Share a Google Drive file and email the link with a message to recipients.

## Steps

1. Find the file: `gws drive files list --params '{"q": "name = '\''Quarterly Report'\''"}'`
2. **Validate** the response contains at least one result with a `fileId` before proceeding. If no results are returned, stop and inform the user the file could not be found.
3. Share the file: `gws drive permissions create --params '{"fileId": "FILE_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "client@example.com"}'`
4. **Validate** the permission creation response confirms success (e.g., returns a permission object with an `id`). If sharing failed, stop and report the error before attempting to email the link.
5. Email the link: `gws gmail +send --to client@example.com --subject 'Quarterly Report' --body 'Hi, please find the report here: https://docs.google.com/document/d/FILE_ID'`

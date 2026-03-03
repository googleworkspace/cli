---
name: recipe-upload-and-share
version: 1.0.0
description: "USE WHEN the user needs to upload a file to Drive and share it with someone."
metadata:
  openclaw:
    category: "recipe"
    domain: "documents"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Upload a File and Share It

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

USE WHEN the user needs to upload a file to Drive and share it with someone.

## Steps

1. Upload the file: `gws drive +upload report.pdf`
2. Note the file ID from the upload output
3. Share with a user: `gws drive permissions create --params '{"fileId": "FILE_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "user@company.com"}'`
4. Optionally announce in Chat: `gws workflow +file-announce --file-id FILE_ID --space spaces/SPACE_ID`


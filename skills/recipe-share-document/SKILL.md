---
name: recipe-share-document
version: 1.0.0
description: "USE WHEN the user needs to share a Drive document with one or more people."
metadata:
  openclaw:
    category: "recipe"
    domain: "documents"
    requires:
      bins: ["gws"]
      skills: ["gws-drive"]
---

# Share a Document with Specific People

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`

USE WHEN the user needs to share a Drive document with one or more people.

> [!CAUTION]
> Choose the correct role (reader, writer, commenter) for the intended access level.

## Steps

1. Find the document: `gws drive files list --params '{"q": "name = '\''Document Name'\''"}'`
2. Share with a user: `gws drive permissions create --params '{"fileId": "FILE_ID"}' --json '{"role": "writer", "type": "user", "emailAddress": "user@company.com"}'`
3. Verify permissions: `gws drive permissions list --params '{"fileId": "FILE_ID"}'`


---
name: recipe-search-and-export-emails
version: 1.0.0
description: "Find Gmail messages matching a query and export them for review."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail"]
---

# Search and Export Emails

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`

Find Gmail messages matching a query and export them for review.

## Steps

1. Search for emails: `gws gmail users messages list --params '{"userId": "me", "q": "from:client@example.com after:2024/01/01"}'`
2. Get full message: `gws gmail users messages get --params '{"userId": "me", "id": "MSG_ID"}'`
3. Export results: `gws gmail users messages list --params '{"userId": "me", "q": "label:project-x"}' --format json > project-emails.json`


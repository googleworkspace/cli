---
name: recipe-send-personalized-emails
version: 1.0.0
description: "Read recipient data from Google Sheets and send personalized Gmail messages to each row."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets", "gws-gmail"]
---

# Send Personalized Emails from a Sheet

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`, `gws-gmail`

Read recipient data from Google Sheets and send personalized Gmail messages to each row.

## Steps

1. Read recipient list: `gws sheets +read --spreadsheet-id SHEET_ID --range 'Contacts!A2:C'`
2. For each row, send a personalized email: `gws gmail +send --to recipient@example.com --subject 'Hello, Name' --body 'Hi Name, your report is ready.'`


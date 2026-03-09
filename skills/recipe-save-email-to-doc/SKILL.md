---
name: recipe-save-email-to-doc
version: 1.0.0
description: "Saves a Gmail message body into a Google Doc for archival, reference, or backup. Use when the user wants to save an email to Google Docs, archive a Gmail message, export or copy an email to a doc, backup a message, or preserve email content in a document. Covers searching for messages by query, retrieving full message content, creating a new Google Doc, and writing the email body into it."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail", "gws-docs"]
---

# Save a Gmail Message to Google Docs

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`, `gws-docs`

Save a Gmail message body into a Google Doc for archival or reference.

## Steps

1. Find the message: `gws gmail users messages list --params '{"userId": "me", "q": "subject:important from:boss@company.com"}' --format table`
   - If the results list is empty, stop and inform the user that no messages matched the query before proceeding.
2. Get message content: `gws gmail users messages get --params '{"userId": "me", "id": "MSG_ID"}'`
3. Create a doc with the content: `gws docs documents create --json '{"title": "Saved Email - Important Update"}'`
   - Verify the response includes a valid `documentId` before continuing. If creation fails, do not attempt to write content.
4. Write the email body: `gws docs +write --document-id DOC_ID --text 'From: boss@company.com
Subject: Important Update

[EMAIL BODY]'`

---
name: recipe-batch-reply-to-emails
version: 1.0.0
description: "Find Gmail messages matching a query and send a standard reply to each one."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail"]
---

# Batch Reply to Similar Gmail Messages

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`

Find Gmail messages matching a query and send a standard reply to each one.

## Steps

1. Find messages needing replies: `gws gmail users messages list --params '{"userId": "me", "q": "is:unread from:customers label:support"}' --format table`
2. Read a message: `gws gmail users messages get --params '{"userId": "me", "id": "MSG_ID"}'`
3. Send a reply: `gws gmail +send --to sender@example.com --subject 'Re: Your Request' --body 'Thank you for reaching out. We have received your request and will respond within 24 hours.'`
4. Mark as read: `gws gmail users messages modify --params '{"userId": "me", "id": "MSG_ID"}' --json '{"removeLabelIds": ["UNREAD"]}'`


---
name: recipe-forward-labeled-emails
version: 1.0.0
description: "Finds Gmail messages with a specific label and forwards them to another email address. Use when the user wants to forward labeled Gmail emails, auto-forward filtered messages, send labeled emails to another address, or set up a filter-and-forward workflow using Gmail labels."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail"]
---

# Forward Labeled Gmail Messages

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`

Find Gmail messages with a specific label and forward them to another address.

## Steps

1. Find labeled messages: `gws gmail users messages list --params '{"userId": "me", "q": "label:needs-review"}' --format table`
2. Get message content: `gws gmail users messages get --params '{"userId": "me", "id": "MSG_ID"}'`
3. Forward via new email: `gws gmail +send --to manager@company.com --subject 'FW: [Original Subject]' --body 'Forwarding for your review:

[Original Message Body]'`

4. **Multiple messages:** If step 1 returns more than one result, repeat steps 2–3 for each message ID in the list, iterating through all returned `MSG_ID` values before moving on.
5. **Validate:** Confirm the forward was sent successfully by checking the response status from step 3, or verify the message appears in the sent folder: `gws gmail users messages list --params '{"userId": "me", "q": "in:sent subject:FW:"}' --format table`

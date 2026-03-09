---
name: recipe-label-and-archive-emails
version: 1.0.0
description: "Apply Gmail labels to matching messages and archive them to keep your inbox clean. Use when the user wants to organize Gmail, apply labels, filter emails, sort messages, clean up their inbox, or work toward inbox zero. Handles email organization, inbox management, email rules, and bulk archiving workflows."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail"]
---

# Label and Archive Gmail Threads

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`

Apply Gmail labels to matching messages and archive them to keep your inbox clean.

## Steps

1. Search for matching emails: `gws gmail users messages list --params '{"userId": "me", "q": "from:notifications@service.com"}' --format table`
2. **Review the results** to confirm these are the messages you want to modify before proceeding. Check the count and message subjects to avoid unintended bulk changes.
3. Apply a label: `gws gmail users messages modify --params '{"userId": "me", "id": "MESSAGE_ID"}' --json '{"addLabelIds": ["LABEL_ID"]}'`
4. Archive (remove from inbox): `gws gmail users messages modify --params '{"userId": "me", "id": "MESSAGE_ID"}' --json '{"removeLabelIds": ["INBOX"]}'`
5. For multiple messages, repeat steps 3–4 for each `MESSAGE_ID` returned in step 1. Verify each modify call succeeds before continuing to the next message.

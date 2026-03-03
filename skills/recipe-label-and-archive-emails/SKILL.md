---
name: recipe-label-and-archive-emails
version: 1.0.0
description: "Apply Gmail labels to matching messages and archive them to keep your inbox clean."
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
2. Apply a label: `gws gmail users messages modify --params '{"userId": "me", "id": "MESSAGE_ID"}' --json '{"addLabelIds": ["LABEL_ID"]}'`
3. Archive (remove from inbox): `gws gmail users messages modify --params '{"userId": "me", "id": "MESSAGE_ID"}' --json '{"removeLabelIds": ["INBOX"]}'`


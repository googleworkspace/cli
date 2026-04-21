---
name: recipe-apply-filter-to-existing-emails
description: "Apply a Gmail filter's actions to existing messages (API equivalent of the UI 'apply to matching conversations' box)."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gws
      skills:
        - gws-gmail
---

# Apply a Gmail Filter to Existing Messages

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`

Apply a Gmail filter's actions to existing messages (API equivalent of the UI 'apply to matching conversations' box).

> [!CAUTION]
> batchModify accepts up to 1000 ids per request — paginate list and modify for larger result sets. `forward` actions on a filter cannot be replayed against past messages.

## Steps

1. Read the filter's criteria and action: `gws gmail users settings filters get --params '{"userId": "me", "id": "FILTER_ID"}'`
2. List matching messages, translating the criteria to a Gmail query (e.g. `subject:"..."`, `from:...`): `gws gmail users messages list --params '{"userId": "me", "q": "subject:\"invoice\" in:anywhere", "maxResults": 500}'`
3. Apply the filter's actions in bulk: `gws gmail users messages batchModify --params '{"userId": "me"}' --json '{"ids": ["MSG_ID_1", "MSG_ID_2"], "addLabelIds": ["LABEL_ID"], "removeLabelIds": ["INBOX", "UNREAD"]}'`
4. Verify the inbox is clear of matches: `gws gmail users messages list --params '{"userId": "me", "q": "subject:\"invoice\" in:inbox"}' --format table`


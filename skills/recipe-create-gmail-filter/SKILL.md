---
name: recipe-create-gmail-filter
version: 1.0.0
description: "Creates a Gmail filter to automatically label, star, archive, or categorize incoming messages. Use when the user wants to organize Gmail, set up email rules, auto-sort messages, filter by sender or subject, or mentions Gmail filters or email automation."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail"]
---

# Create a Gmail Filter

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`

Create a Gmail filter to automatically label, star, or categorize incoming messages.

## Steps

1. List existing labels: `gws gmail users labels list --params '{"userId": "me"}' --format table`
2. Create a new label: `gws gmail users labels create --params '{"userId": "me"}' --json '{"name": "Receipts"}'`
   > **Note:** Note the `id` field from the response — this is the `LABEL_ID` value to use in the next step.
3. Create a filter: `gws gmail users settings filters create --params '{"userId": "me"}' --json '{"criteria": {"from": "receipts@example.com"}, "action": {"addLabelIds": ["LABEL_ID"], "removeLabelIds": ["INBOX"]}}'`
4. Verify filter: `gws gmail users settings filters list --params '{"userId": "me"}' --format table`

## Troubleshooting

- **Invalid label ID:** Ensure the `LABEL_ID` value is copied exactly from the `id` field returned in step 2. System labels (e.g., `INBOX`, `SPAM`) use uppercase strings; user-created labels use a numeric ID.
- **Filter already exists:** If the API returns a conflict error, list existing filters (step 4) to check whether an identical filter is already in place before creating a new one.

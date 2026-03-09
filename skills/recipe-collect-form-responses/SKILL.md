---
name: recipe-collect-form-responses
version: 1.0.0
description: "Retrieves and reviews responses from a Google Form using the gws CLI. Use when a user wants to check Google Form responses, view form submissions, access survey results, read form answers, or inspect Google Forms data. Supports listing available forms, fetching form details, and retrieving all submissions in a formatted table."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-forms"]
---

# Check Form Responses

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-forms`

Retrieve and review responses from a Google Form.

## Steps

1. List forms: `gws forms forms list` (if you don't have the form ID)
   - **Validate:** Confirm the target form appears in the output and note its `formId` before proceeding.
2. Get form details: `gws forms forms get --params '{"formId": "FORM_ID"}'`
   - **Validate:** Confirm the returned form title and structure match the expected form before fetching responses.
3. Get responses: `gws forms forms responses list --params '{"formId": "FORM_ID"}' --format table`
   - **Expected output:** A table with columns such as `responseId`, `createTime`, and `answers`. An empty table means no submissions yet.

## Troubleshooting

- **Invalid form ID:** Double-check the form ID returned by `gws forms forms list` and ensure it is copied exactly.
- **Permission errors:** Confirm the authenticated account has at least viewer access to the target form.

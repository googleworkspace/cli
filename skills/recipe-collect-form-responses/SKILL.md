---
name: recipe-collect-form-responses
description: "Retrieve and review responses from a Google Form."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gws
      skills:
        - gws-forms
        - gws-drive
---

# Check Form Responses

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-forms`, `gws-drive`

Retrieve and review responses from a Google Form.

## Steps

1. Find your form ID: `gws drive files list --params "{\"q\": \"mimeType = 'application/vnd.google-apps.form' and trashed = false\"}" --format table`
2. Get form details: `gws forms forms get --params '{"formId": "FORM_ID"}'`
3. Get responses: `gws forms forms responses list --params '{"formId": "FORM_ID"}' --format table`


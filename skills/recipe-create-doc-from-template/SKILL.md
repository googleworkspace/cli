---
name: recipe-create-doc-from-template
version: 1.0.0
description: "Copies a Google Docs template, fills in content, and shares the document with collaborators. Use when the user wants to create a Google document or gdoc from a template, duplicate a Google Doc, set up document collaboration, or share access with team members. Supports workflows involving doc templates, Google document creation, and document sharing."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-drive", "gws-docs"]
---

# Create a Google Doc from a Template

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-drive`, `gws-docs`

Copy a Google Docs template, fill in content, and share with collaborators.

## Steps

1. Copy the template: `gws drive files copy --params '{"fileId": "TEMPLATE_DOC_ID"}' --json '{"name": "Project Brief - Q2 Launch"}'`
2. Validate the copy succeeded: confirm the response contains a file `id` field. If no `id` is returned, stop and report the error (e.g., invalid template ID or insufficient access to the source file).
3. Get the new doc ID from the response.
4. Add content: `gws docs +write --document-id NEW_DOC_ID --text '## Project: Q2 Launch

### Objective
Launch the new feature by end of Q2.'`
5. Share with team: `gws drive permissions create --params '{"fileId": "NEW_DOC_ID"}' --json '{"role": "writer", "type": "user", "emailAddress": "team@company.com"}'`
6. Validate sharing succeeded: confirm the response includes a `kind` of `drive#permission`. If a permission-denied error is returned, inform the user they may lack sharing rights on the document.

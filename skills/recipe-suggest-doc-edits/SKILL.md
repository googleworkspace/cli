---
name: recipe-suggest-doc-edits
description: "Propose edits on a Google Doc as review comments quoting the target text (the Docs API can't create suggestions)."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gws
      skills:
        - gws-docs
        - gws-drive
---

# Suggest Edits on a Google Doc via Comments

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-docs`, `gws-drive`

Propose edits on a Google Doc as review comments quoting the target text (the Docs API can't create suggestions).

## Steps

1. Note the API limitation: `documents.batchUpdate` makes direct edits only — tracked "Suggesting-mode" suggestions are UI-only (`suggestionsViewMode` exists only on the read path). Propose edits as comments instead.
2. Read the document to capture the exact current text to change: `gws docs documents get --params '{"documentId": "DOC_ID"}'`
3. Post each proposed edit as a comment quoting the target text: `gws drive comments create --params '{"fileId": "DOC_ID", "fields": "id"}' --json '{"content": "SUGGESTED EDIT — replace with: NEW TEXT", "quotedFileContent": {"value": "EXACT CURRENT TEXT"}}'`
4. After reviewers accept, apply the edit for real: `gws docs documents batchUpdate --params '{"documentId": "DOC_ID"}' --json '{"requests": [{"replaceAllText": {"containsText": {"text": "EXACT CURRENT TEXT", "matchCase": true}, "replaceText": "NEW TEXT"}}]}'`
5. Resolve the comment: `gws drive replies create --params '{"fileId": "DOC_ID", "commentId": "COMMENT_ID", "fields": "id"}' --json '{"action": "resolve", "content": "Applied."}'`


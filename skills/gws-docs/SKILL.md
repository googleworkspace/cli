---
name: gws-docs
version: 1.0.0
description: "Manages Google Docs documents via the gws CLI, supporting create, get, and batchUpdate operations to build, read, and modify document content and formatting. Use when a user wants to create a Google Doc, edit or format a Google document, read a gdocs file, retrieve document content from a docs.google.com link, or apply structured changes to a Google Drive document programmatically."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws docs --help"
---

# docs (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws docs <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+write`](../gws-docs-write/SKILL.md) | Append text to a document |

## API Resources

### documents

  - `batchUpdate` — Applies one or more updates to the document. Each request is validated before being applied. If any request is not valid, then the entire request will fail and nothing will be applied. Some requests have replies to give you some information about how they are applied. Other requests do not need to return information; these each return an empty reply. The order of replies matches that of the requests.
  - `create` — Creates a blank document using the title given in the request. Other fields in the request, including any provided content, are ignored. Returns the created document.
  - `get` — Gets the latest version of the specified document.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws docs --help

# Inspect a method's required params, types, and defaults
gws schema docs.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Examples

### Create a document

```bash
gws docs documents create --json '{"title": "My New Document"}'
```

### Get a document

```bash
gws docs documents get --params '{"documentId": "YOUR_DOCUMENT_ID"}'
```

### batchUpdate — insert text at a location

```bash
gws docs documents batchUpdate \
  --params '{"documentId": "YOUR_DOCUMENT_ID"}' \
  --json '{
    "requests": [
      {
        "insertText": {
          "location": { "index": 1 },
          "text": "Hello, world!\n"
        }
      }
    ]
  }'
```

### batchUpdate — apply bold formatting to a range

```bash
gws docs documents batchUpdate \
  --params '{"documentId": "YOUR_DOCUMENT_ID"}' \
  --json '{
    "requests": [
      {
        "updateTextStyle": {
          "range": { "startIndex": 1, "endIndex": 14 },
          "textStyle": { "bold": true },
          "fields": "bold"
        }
      }
    ]
  }'
```

## Validation Notes

For `batchUpdate`, each request in the batch is validated individually before any are applied — if a single request is invalid, the entire batch is rejected. To minimise failures:
1. Run `gws schema docs.documents.batchUpdate` to confirm required fields and types for each request kind.
2. Build and verify your request payload structure before submitting.
3. Group related changes together, but keep unrelated changes in separate `batchUpdate` calls to limit blast radius if validation fails.

**If a `batchUpdate` call fails:**
1. Check the error message for the invalid request index (e.g. `requests[2]`).
2. Fix the specific request — verify required fields, index ranges, and types against `gws schema docs.documents.batchUpdate`.
3. Retry the entire batch once corrected (no partial state is left to clean up, since nothing was applied).

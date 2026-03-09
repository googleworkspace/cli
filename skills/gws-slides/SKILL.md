---
name: gws-slides
version: 1.0.0
description: "Google Slides: Creates, reads, and edits Google Slides presentations (slide decks, slideshows, Google presentations). Use when the user mentions Google Slides, a slide deck, or needs to create slides, add speaker notes, modify layouts, insert images or charts, apply batch updates, or read presentation content in Google Workspace."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws slides --help"
---

# slides (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws slides <resource> <method> [flags]
```

## API Resources

### presentations

  - `batchUpdate` — Applies one or more updates to the presentation. Each request is validated before being applied. If any request is not valid, then the entire request will fail and nothing will be applied. Some requests have replies to give you some information about how they are applied. Other requests do not need to return information; these each return an empty reply. The order of replies matches that of the requests.
  - `create` — Creates a blank presentation using the title given in the request. If a `presentationId` is provided, it is used as the ID of the new presentation. Otherwise, a new ID is generated. Other fields in the request, including any provided content, are ignored. Returns the created presentation.
  - `get` — Gets the latest version of the specified presentation.
  - `pages` — Operations on the 'pages' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws slides --help

# Inspect a method's required params, types, and defaults
gws schema slides.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Examples

### Get a presentation

```bash
gws slides presentations get --params 'presentationId=1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgVE2upms'
```

### Create a new presentation

```bash
gws slides presentations create --json '{"title": "Q3 Roadmap"}'
```

### Batch update — add a new slide with a title

```bash
gws slides presentations batchUpdate \
  --params 'presentationId=1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgVE2upms' \
  --json '{
    "requests": [
      {
        "insertText": {
          "objectId": "SLIDE_OBJECT_ID",
          "insertionIndex": 0,
          "text": "My New Slide Title"
        }
      }
    ]
  }'
```

> **Safety note:** `batchUpdate` is atomic — if any single request in the batch is invalid, the entire update fails and no changes are applied. Inspect available request types and their schemas with `gws schema slides.presentations.batchUpdate` before building complex update payloads.

> **Verification:** After a successful `batchUpdate`, confirm the changes took effect by fetching the presentation:
> ```bash
> gws slides presentations get --params 'presentationId=PRESENTATION_ID'
> ```
> If the update failed, review the error message to identify which request was invalid, correct the payload, and retry the full batch.

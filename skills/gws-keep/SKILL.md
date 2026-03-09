---
name: gws-keep
version: 1.0.0
description: "Create, read, delete, list, and manage Google Keep notes and their permissions via the gws CLI. Use when the user mentions Google Keep, Keep notes, note-taking, checklists, reminders, or wants to create, retrieve, delete, share, or paginate through notes and attachments."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws keep --help"
---

# keep (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws keep <resource> <method> [flags]
```

## API Resources

### media

  - `download` — Gets an attachment. To download attachment media via REST requires the alt=media query parameter. Returns a 400 bad request error if attachment media is not available in the requested MIME type.

### notes

  - `create` — Creates a new note.
  - `delete` — Deletes a note. Caller must have the `OWNER` role on the note to delete. Deleting a note removes the resource immediately and cannot be undone. Any collaborators will lose access to the note.
  - `get` — Gets a note.
  - `list` — Lists notes. Every list call returns a page of results with `page_size` as the upper bound of returned items. A `page_size` of zero allows the server to choose the upper bound. The ListNotesResponse contains at most `page_size` entries. If there are more things left to list, it provides a `next_page_token` value. (Page tokens are opaque values.) To get the next page of results, copy the result's `next_page_token` into the next request's `page_token`.
  - `permissions` — Operations on the 'permissions' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws keep --help

# Inspect a method's required params, types, and defaults
gws schema keep.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Examples

**Create a new note:**
```bash
gws keep notes create --json '{"title": "Shopping List", "body": {"text": {"text": "Milk, Eggs, Bread"}}}'
```

**List notes (first page):**
```bash
gws keep notes list --params 'page_size=10'
```

**List notes (subsequent page using next_page_token):**
```bash
# Copy next_page_token from the previous response into page_token
gws keep notes list --params 'page_size=10&page_token=<next_page_token>'
```

**Get a specific note:**
```bash
gws keep notes get --params 'name=notes/<note_id>'
```

**Delete a note:**

> ⚠️ **Irreversible.** Deletion removes the note immediately and permanently — it cannot be undone and all collaborators instantly lose access. Always confirm the correct `note_id` with the user before proceeding. When in doubt, fetch the note first with `notes get` and display its title/content for user confirmation.

```bash
# Verify before deleting
gws keep notes get --params 'name=notes/<note_id>'

# Then delete only after confirmation
gws keep notes delete --params 'name=notes/<note_id>'
```

## Error Handling

| Situation | Likely cause | Action |
|---|---|---|
| `400 Bad Request` on media download | Attachment not available in requested MIME type | Check supported MIME types via `gws schema keep.media.download` |
| `403 Forbidden` on delete | Caller does not have `OWNER` role on the note | Inform the user they lack ownership; do not retry |
| `404 Not Found` | Note ID is invalid or already deleted | Verify the ID with `notes list` before retrying |
| Paginated list returns no `next_page_token` | Final page of results reached | Stop pagination; all results have been returned |

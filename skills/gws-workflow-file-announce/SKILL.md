---
name: gws-workflow-file-announce
version: 1.0.0
description: "Google Workflow: Announce, share, or post a Google Drive file to a Google Chat space to notify team members. Use when the user wants to share a Drive file in a Chat channel, post a file link to a space, notify teammates about an uploaded file, or send a file announcement to Google Chat."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws workflow +file-announce --help"
---

# workflow +file-announce

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Announce a Drive file in a Chat space

## Usage

```bash
gws workflow +file-announce --file-id <ID> --space <SPACE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--file-id` | ✓ | — | Drive file ID to announce |
| `--space` | ✓ | — | Chat space name (e.g. spaces/SPACE_ID) |
| `--message` | — | — | Custom announcement message |
| `--format` | — | — | Output format: json (default), table, yaml, csv |

## Examples

```bash
gws workflow +file-announce --file-id FILE_ID --space spaces/ABC123
gws workflow +file-announce --file-id FILE_ID --space spaces/ABC123 --message 'Check this out!'
```

## Upload-then-Announce Workflow

When sharing a newly uploaded file, follow this sequence:

1. **Upload the file** to Drive:
   ```bash
   gws drive +upload --file /path/to/file.pdf
   ```
2. **Verify the upload succeeded** — confirm a file ID is returned in the output.
3. **Announce the file** to the target Chat space using the returned file ID:
   ```bash
   gws workflow +file-announce --file-id <RETURNED_FILE_ID> --space spaces/ABC123
   ```
4. **Verify the announcement** — check the command output confirms the Chat message was sent (look for a message ID or success status in the response).

### Common errors
- **Invalid space ID** — ensure the `--space` value matches the full space name format (e.g. `spaces/SPACE_ID`). Use `gws chat +list-spaces` to look up valid space names.
- **Missing file permissions** — the authenticated account must have at least read access to the Drive file before it can be announced.

## Tips

- This is a write command — sends a Chat message.
- Use `gws drive +upload` first to upload the file, then announce it here.
- Fetches the file name from Drive to build the announcement.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-workflow](../gws-workflow/SKILL.md) — All cross-service productivity workflows commands

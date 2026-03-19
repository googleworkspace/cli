---
name: gws-docs-revisions
version: 1.0.0
description: "Google Docs: List revision history for a document."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws docs +revisions --help"
---

# docs +revisions

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

List revision history for a Google Docs document.

## Usage

```bash
gws docs +revisions --document <ID> [--limit <N>]
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--document` | ✓ | — | Document ID (from the URL) |
| `--limit` | — | 20 | Maximum number of revisions to return (1–1000) |

## Examples

```bash
# Show last 20 revisions (default)
gws docs +revisions --document 1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgVE2upms

# Show last 5 revisions
gws docs +revisions --document 1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgVE2upms --limit 5
```

## Output

Each revision includes:

| Field | Description |
|-------|-------------|
| `id` | Revision ID |
| `modifiedTime` | When this revision was created |
| `lastModifyingUser.displayName` | Who made the change |
| `keepForever` | Whether this revision is pinned (won't be auto-deleted) |
| `size` | Size of the revision in bytes |

## Limitations

> [!IMPORTANT]
> **Content is not available.** The Google Drive API returns revision *metadata* only for
> native Google Docs files. The actual text content of past revisions cannot be retrieved
> via API — only the current content is accessible via `gws docs documents get`.
>
> To read a specific revision's content, open the document in Google Docs and use
> **File → Version history → See version history**.

## Scope

This command uses the `drive.readonly` OAuth scope. No write access is required.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-docs](../gws-docs/SKILL.md) — All Google Docs commands
- [gws-docs-write](../gws-docs-write/SKILL.md) — Append text to a document

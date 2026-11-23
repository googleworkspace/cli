---
name: gws-forms-responses
description: "Google Forms: A form's responses: one row each, question titles as columns."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws forms +responses --help"
---

# forms +responses

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

A form's responses: one row each, question titles as columns

## Usage

```bash
gws forms +responses <form>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<form>` | ✓ | — | Form ID, or the form's edit URL (docs.google.com/forms/d/<id>/edit) |
| `--since` | — | — | Only responses submitted at or after this date (YYYY-MM-DD, local midnight) or RFC 3339 time |

## Examples

```bash
gws forms +responses 1FAIpQLSf...edit-id
gws forms +responses https://docs.google.com/forms/d/<id>/edit --format csv > responses.csv
gws forms +responses <id> --since 2026-11-01
gws forms +responses <id> --since 2026-11-20T09:00:00+01:00 --format json
```

## Tips

- Read-only. Default output is a table; csv and json keep every column.
- Columns: submitted, email (if collected), score (if a quiz), then one per
- question. A grid question gives one column per row: 'Question [row]'.
- Several answers to one question are joined with '; '.
- The ID in a /forms/d/e/.../viewform link is the public one and won't work:
- open the form in edit mode and copy that URL instead.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-forms](../gws-forms/SKILL.md) — All read and write google forms commands

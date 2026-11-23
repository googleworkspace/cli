---
name: gws-docs-export
description: "Google Docs: Export a Doc as Markdown, plain text or HTML."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws docs +export --help"
---

# docs +export

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Export a Doc as Markdown, plain text or HTML

## Usage

```bash
gws docs +export <doc>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<doc>` | ✓ | — | Doc id, URL or Drive path |
| `--output` | — | — | Write to this file instead of stdout |

## Examples

```bash
gws docs +export DOC_ID
gws docs +export 'Company/strategy/Q1 plan' --output plan.md
gws docs +export https://docs.google.com/document/d/ID/edit --format txt
gws docs +export DOC_ID --format html -o doc.html
```

## Tips

- --format takes md (default), txt or html.
- Markdown keeps headings, lists, bold, italic, links, tables and code.
- Images become links: with --output they are saved next to the file in
- <name>_images/; on stdout they link to the image URLs Docs reports, which
- expire after a while.
- Documents too large for Drive's export (10 MB) are converted from the
- Docs API instead.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-docs](../gws-docs/SKILL.md) — All read and write google docs commands

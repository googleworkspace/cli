---
name: gws-docs-write
description: "Google Docs: Write Markdown into a Doc as real formatting (or append plain text)."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws docs +write --help"
---

# docs +write

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Write Markdown into a Doc as real formatting (or append plain text)

## Usage

```bash
gws docs +write
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<file>` | — | — | Markdown file to write ('-' reads stdin) |
| `--document` | — | — | Existing Doc: id, URL or Drive path. With a Markdown file, its body is replaced |
| `--new` | — | — | Create a new Doc with this title |
| `--folder` | — | — | Folder for --new: id, URL or Drive path (default: My Drive) |
| `--text` | — | — | Append this plain text to --document instead of writing Markdown |

## Examples

```bash
gws docs +write plan.md --new 'Q1 plan'
gws docs +write plan.md --new 'Q1 plan' --folder 'Company/strategy'
gws docs +write plan.md --doc DOC_ID
gws docs +write plan.md --doc 'Company/strategy/Q1 plan'
cat notes.md | gws docs +write - --doc https://docs.google.com/document/d/ID/edit
gws docs +write --document DOC_ID --text 'Appended line'
```

## Tips

- Headings, nested lists, bold, italic, strikethrough, links, code (as
- monospace), tables and images become real Docs formatting; Drive's own
- Markdown converter does the conversion.
- --doc replaces the body of the existing Doc in place: its id, link,
- sharing and version history stay.
- Local images (relative to the Markdown file) are embedded; remote image
- URLs must be publicly reachable.
- --text keeps the old behaviour: it appends plain text to --document.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-docs](../gws-docs/SKILL.md) — All read and write google docs commands

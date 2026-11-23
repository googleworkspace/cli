---
name: gws-meet-notes
description: "Google Meet: Print Gemini 'Take notes for me' notes for a meeting."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws meet +notes --help"
---

# meet +notes

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Print Gemini "Take notes for me" notes for a meeting

## Usage

```bash
gws meet +notes <record>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<record>` | ✓ | — | conferenceRecords/ID, a meeting code (abc-defg-hij), a Meet link, or "latest" |

## Examples

```bash
gws meet +notes latest
gws meet +notes abc-defg-hij --format json
```

## Tips

- Reads Meet smart notes first, then the notes Doc attached to the Calendar event.
- Exits non-zero when no notes exist.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-meet](../gws-meet/SKILL.md) — All manage google meet conferences commands

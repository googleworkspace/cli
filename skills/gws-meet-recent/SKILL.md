---
name: gws-meet-recent
description: "Google Meet: List recent meetings with title, code, duration and head count."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws meet +recent --help"
---

# meet +recent

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

List recent meetings with title, code, duration and head count

## Usage

```bash
gws meet +recent
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--days` | — | 7 | How many days back to look |
| `--limit` | — | 20 | Maximum number of meetings to list |

## Examples

```bash
gws meet +recent
gws meet +recent --days 30 --limit 5 --format json
```

## Tips

- Newest first. Titles come from your primary Calendar, matched by meeting code.
- Use the record or meeting code with +attendees, +transcript, +notes, +recording.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-meet](../gws-meet/SKILL.md) — All manage google meet conferences commands

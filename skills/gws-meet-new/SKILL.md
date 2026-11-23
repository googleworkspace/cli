---
name: gws-meet-new
description: "Google Meet: Create a meeting link, optionally with a Calendar event."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws meet +new --help"
---

# meet +new

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Create a meeting link, optionally with a Calendar event

## Usage

```bash
gws meet +new
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--title` | — | — | Also create a Calendar event with this title holding the link |
| `--start` | — | — | Event start (RFC 3339). Default: now |
| `--minutes` | — | — | Event length in minutes. Default: 30 |

## Examples

```bash
gws meet +new
gws meet +new --title 'Design review'
gws meet +new --title 'Standup' --start 2026-06-17T09:00:00-07:00 --minutes 15
```

## Tips

- The event goes on your primary calendar with no guests; nobody is invited.
- Use --dry-run to see the requests without creating anything.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-meet](../gws-meet/SKILL.md) — All manage google meet conferences commands

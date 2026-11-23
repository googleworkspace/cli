---
name: gws-youtube-videos
description: "Google YouTube Data API v3: List the channel's most recent uploads with views and status."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws youtube +videos --help"
---

# youtube +videos

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

List the channel's most recent uploads with views and status

## Usage

```bash
gws youtube +videos
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--limit` | — | 10 | How many videos (default 10, max 500) |

## Examples

```bash
gws youtube +videos
gws youtube +videos --limit 50 --format csv
```

## Tips

- Read-only. Includes private and unlisted uploads of the signed-in channel.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-youtube](../gws-youtube/SKILL.md) — All upload and manage youtube videos, playlists and channels commands

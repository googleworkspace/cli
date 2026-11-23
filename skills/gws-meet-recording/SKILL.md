---
name: gws-meet-recording
description: "Google Meet: List a meeting's recordings and optionally download them."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws meet +recording --help"
---

# meet +recording

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

List a meeting's recordings and optionally download them

## Usage

```bash
gws meet +recording <record>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<record>` | ✓ | — | conferenceRecords/ID, a meeting code (abc-defg-hij), a Meet link, or "latest" |
| `--download` | — | — | Download generated recordings into this directory (relative to the current directory) |

## Examples

```bash
gws meet +recording latest
gws meet +recording abc-defg-hij --download recordings
```

## Tips

- Files already downloaded at full size are skipped.
- --download must be a relative path inside the current directory.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-meet](../gws-meet/SKILL.md) — All manage google meet conferences commands

---
name: gws-meet-transcript
description: "Google Meet: Print a meeting transcript as speaker-labelled text."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws meet +transcript --help"
---

# meet +transcript

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Print a meeting transcript as speaker-labelled text

## Usage

```bash
gws meet +transcript <record>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<record>` | ✓ | — | conferenceRecords/ID, a meeting code (abc-defg-hij), a Meet link, or "latest" |

## Examples

```bash
gws meet +transcript latest
gws meet +transcript abc-defg-hij --format json
```

## Tips

- Output is plain text by default (--format text); use --format json for entries.
- Falls back to exporting the transcript Doc when the API holds no entries.
- Exits non-zero when the meeting has no transcript.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-meet](../gws-meet/SKILL.md) — All manage google meet conferences commands

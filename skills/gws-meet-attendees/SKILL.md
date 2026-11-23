---
name: gws-meet-attendees
description: "Google Meet: List who joined a meeting, when, and for how long."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws meet +attendees --help"
---

# meet +attendees

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

List who joined a meeting, when, and for how long

## Usage

```bash
gws meet +attendees <record>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<record>` | ✓ | — | conferenceRecords/ID, a meeting code (abc-defg-hij), a Meet link, or "latest" |

## Examples

```bash
gws meet +attendees latest
gws meet +attendees abc-defg-hij
gws meet +attendees conferenceRecords/ID --format json
```

## Tips

- Total time adds up every session, so rejoining is counted correctly.
- A meeting code resolves to its most recent conference.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-meet](../gws-meet/SKILL.md) — All manage google meet conferences commands

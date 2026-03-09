---
name: gws-calendar-agenda
version: 1.0.0
description: "Google Calendar agenda viewer: lists upcoming events, meetings, and appointments across all calendars using the gws CLI. Use when the user wants to check their schedule, see what's on their calendar, view today's or tomorrow's events, browse this week's agenda, or ask 'what do I have today?', 'what meetings are coming up?', or 'show my appointments'. Supports filtering by calendar name/ID and adjustable day ranges. Read-only — never creates or modifies events."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws calendar +agenda --help"
---

# calendar +agenda

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Show upcoming events across all calendars

## Usage

```bash
gws calendar +agenda
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--today` | — | — | Show today's events |
| `--tomorrow` | — | — | Show tomorrow's events |
| `--week` | — | — | Show this week's events |
| `--days` | — | — | Number of days ahead to show |
| `--calendar` | — | — | Filter to specific calendar name or ID |

## Examples

```bash
gws calendar +agenda
gws calendar +agenda --today
gws calendar +agenda --week --format table
gws calendar +agenda --days 3 --calendar 'Work'
```

## Tips

- Read-only — never modifies events.
- Queries all calendars by default; use --calendar to filter.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-calendar](../gws-calendar/SKILL.md) — All manage calendars and events commands

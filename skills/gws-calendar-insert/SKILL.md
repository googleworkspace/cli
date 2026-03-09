---
name: gws-calendar-insert
version: 1.0.0
description: "Google Calendar: Creates a new calendar event using the gws CLI. Use when a user wants to schedule a meeting, add an appointment, book time, block off time, set up a call, or add something to their Google Calendar (gcal). Supports setting an event title/summary, start and end times (ISO 8601/RFC3339), location, description, and one or more attendees. Common trigger phrases include 'schedule a meeting', 'create an event', 'add to my calendar', 'book time with', or 'set up an appointment'."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws calendar +insert --help"
---

# calendar +insert

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

create a new event

## Usage

```bash
gws calendar +insert --summary <TEXT> --start <TIME> --end <TIME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar` | — | primary | Calendar ID (default: primary) |
| `--summary` | ✓ | — | Event summary/title |
| `--start` | ✓ | — | Start time (ISO 8601, e.g., 2024-01-01T10:00:00Z) |
| `--end` | ✓ | — | End time (ISO 8601) |
| `--location` | — | — | Event location |
| `--description` | — | — | Event description/body |
| `--attendee` | — | — | Attendee email (can be used multiple times) |

## Examples

```bash
gws calendar +insert --summary 'Standup' --start '2026-06-17T09:00:00-07:00' --end '2026-06-17T09:30:00-07:00'
gws calendar +insert --summary 'Review' --start ... --end ... --attendee alice@example.com
```

## Tips

- Use RFC3339 format for times (e.g. 2026-06-17T09:00:00-07:00).
- For recurring events or conference links, use the raw API instead.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-calendar](../gws-calendar/SKILL.md) — All manage calendars and events commands

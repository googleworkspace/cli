---
name: gws-calendar-free
description: "Google Calendar: Find free time slots across your calendars."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws calendar +free --help"
---

# calendar +free

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Find free time slots across your calendars

## Usage

```bash
gws calendar +free
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--days` | — | 3 | Days to search, starting today (default 3, max 60) |
| `--minutes` | — | 30 | Shortest free slot to show, in minutes (default 30) |
| `--between` | — | 09:00-18:00 | Working hours each day, HH:MM-HH:MM (default 09:00-18:00) |
| `--weekdays` | — | — | Skip Saturdays and Sundays |
| `--calendar` | — | — | Only check calendars whose name or ID matches (repeatable) |
| `--count-all-day` | — | — | Let busy all-day events block whole days (by default only out-of-office ones do) |
| `--all-calendars` | — | — | Also count calendars you only subscribe to (default: calendars you own or edit) |
| `--timezone` | — | — | IANA timezone for --between and the output. Defaults to your Google account timezone. |

## Examples

```bash
gws calendar +free
gws calendar +free --days 5 --minutes 60
gws calendar +free --between 10:00-16:30 --weekdays
gws calendar +free --calendar Work --format json
```

## Tips

- Read-only. Timed events block time unless marked 'free' or declined.
- All-day events (postings, trips, reminders) don't block time unless they
- are out-of-office or you pass --count-all-day. Calendars that share only
- free/busy are read through the freeBusy API. Slots that have already
- started today begin at the next quarter hour.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-calendar](../gws-calendar/SKILL.md) — All manage calendars and events commands

---
name: gws-workflow-standup-report
version: 1.0.0
description: "Fetches today's Google Calendar events and open Google Tasks, then formats them as a standup summary report. Use when the user asks for a daily standup, morning briefing, or wants to see today's agenda and to-dos together — e.g. 'give me my standup', 'what's on my schedule and tasks today', 'morning summary', or 'show my Google Calendar and tasks as a standup'."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws workflow +standup-report --help"
---

# workflow +standup-report

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Today's meetings + open tasks as a standup summary

## Usage

```bash
gws workflow +standup-report
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--format` | — | — | Output format: json (default), table, yaml, csv |

## Examples

```bash
gws workflow +standup-report
gws workflow +standup-report --format table
```

### Example Output (table)

```
STANDUP REPORT — 2024-06-10

MEETINGS
  09:00–09:30  Daily Sync         (Google Meet)
  11:00–12:00  Product Review     (Conference Room B)
  15:30–16:00  1:1 with Manager   (Google Meet)

OPEN TASKS
  [ ] Finish Q2 report draft       (due today)
  [ ] Review PR #482
  [ ] Send follow-up to design team
```

### Example Output (json, truncated)

```json
{
  "date": "2024-06-10",
  "meetings": [
    { "start": "09:00", "end": "09:30", "title": "Daily Sync", "location": "Google Meet" }
  ],
  "tasks": [
    { "title": "Finish Q2 report draft", "due": "2024-06-10", "status": "needsAction" }
  ]
}
```

## Tips

- Read-only — never modifies data.
- Combines calendar agenda (today) with tasks list.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-workflow](../gws-workflow/SKILL.md) — All cross-service productivity workflows commands

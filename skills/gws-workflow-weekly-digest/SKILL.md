---
name: gws-workflow-weekly-digest
version: 1.0.0
description: "Generates a weekly digest using the `gws` CLI by combining this week's calendar events with a Gmail unread email count into a single summary. Use when the user asks for a weekly overview, wants to summarize their week, says 'what's on my calendar this week', 'check my inbox', 'how many unread emails do I have', 'show my schedule', 'week ahead', or needs a combined snapshot of upcoming meetings and Gmail inbox status. Outputs in JSON, table, YAML, or CSV format."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws workflow +weekly-digest --help"
---

# workflow +weekly-digest

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Weekly summary: this week's meetings + unread email count

## Usage

```bash
gws workflow +weekly-digest
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--format` | — | — | Output format: json (default), table, yaml, csv |

## Examples

```bash
gws workflow +weekly-digest
gws workflow +weekly-digest --format table
```

### Example Output (JSON)

```json
{
  "week": "2024-06-10 – 2024-06-16",
  "meetings": [
    { "date": "2024-06-10", "title": "Team Standup", "time": "09:00" },
    { "date": "2024-06-12", "title": "Product Review", "time": "14:00" },
    { "date": "2024-06-14", "title": "1:1 with Manager", "time": "11:00" }
  ],
  "meeting_count": 3,
  "unread_email_count": 42
}
```

### Example Output (table)

```
Week: 2024-06-10 – 2024-06-16
Meetings (3):
  Mon Jun 10  09:00  Team Standup
  Wed Jun 12  14:00  Product Review
  Fri Jun 14  11:00  1:1 with Manager
Unread emails: 42
```

## Tips

- Read-only — never modifies data.
- Combines calendar agenda (week) with gmail triage summary.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-workflow](../gws-workflow/SKILL.md) — All cross-service productivity workflows commands

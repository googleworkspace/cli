---
name: gws-workflow-meeting-prep
version: 1.0.0
description: "Google Workflow: Retrieves upcoming calendar event details, lists attendees, and surfaces linked Google Docs to help prepare for your next meeting. Use when a user asks to prepare for a meeting, run meeting prep, check what's on their calendar, review upcoming meetings, see meeting attendees, or access linked docs or agenda for a Google Calendar event."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws workflow +meeting-prep --help"
---

# workflow +meeting-prep

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Retrieves the next upcoming Google Calendar event and compiles meeting prep details: agenda/description, attendee list, and any linked Google Docs or Drive files.

## Usage

```bash
gws workflow +meeting-prep
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--calendar` | — | primary | Calendar ID (default: primary) |
| `--format` | — | — | Output format: json (default), table, yaml, csv |

## Examples

```bash
gws workflow +meeting-prep
gws workflow +meeting-prep --calendar Work
```

## Example Output

```
Next Meeting: Q3 Planning Sync
Time:        2024-07-15 10:00 – 11:00 (UTC)
Location:    https://meet.google.com/abc-defg-hij

Attendees:
  - alice@example.com (organizer)
  - bob@example.com
  - carol@example.com

Agenda / Description:
  Review Q3 OKRs, assign owners, and agree on launch timeline.

Linked Docs:
  - Q3 OKR Draft (Google Doc): https://docs.google.com/...
  - Launch Timeline (Google Sheet): https://docs.google.com/...
```

When presenting results to the user, surface the meeting time, attendee list, agenda, and linked documents clearly so they can review everything in one place before joining.

## Tips

- Read-only — never modifies data.
- Shows the next upcoming event with attendees and description.
- Linked docs are extracted from the event description and attachments.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-workflow](../gws-workflow/SKILL.md) — All cross-service productivity workflows commands

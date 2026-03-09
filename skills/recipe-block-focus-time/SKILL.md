---
name: recipe-block-focus-time
version: 1.0.0
description: "Creates recurring focus time blocks on Google Calendar to protect deep work hours. Use when a user wants to block their calendar for focus time, schedule deep work sessions, protect meeting-free time, set up productivity blocks, or create do-not-disturb periods. Handles recurring weekly patterns with correct busy/transparency settings. Trigger terms: 'block my calendar', 'schedule focus blocks', 'protect time for concentration', 'meeting-free time', 'deep work hours'."
metadata:
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar"]
---

# Block Focus Time on Google Calendar

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`

Create recurring focus time blocks on Google Calendar to protect deep work hours.

## Steps

1. Create recurring focus block: `gws calendar events insert --params '{"calendarId": "primary"}' --json '{"summary": "Focus Time", "description": "Protected deep work block", "start": {"dateTime": "2025-01-20T09:00:00", "timeZone": "America/New_York"}, "end": {"dateTime": "2025-01-20T11:00:00", "timeZone": "America/New_York"}, "recurrence": ["RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"], "transparency": "opaque"}'`
   - Update `dateTime` values to match the user's actual desired start date and time before running.
   - Adjust `timeZone` to match the user's local timezone if different from `America/New_York`.
2. Verify it shows as busy: `gws calendar +agenda`

## Troubleshooting

- **Event not showing as busy:** Confirm `"transparency": "opaque"` is present in the JSON payload — this is what marks the block as busy to other calendar users. If omitted, the event defaults to `"transparent"` (free).
- **Insert command fails:** Check that the `gws-calendar` skill is loaded and that the account has write permissions to the `primary` calendar.
- **Recurrence not applying:** Verify the `RRULE` string is correctly formatted. Use `BYDAY=MO,TU,WE,TH,FR` for weekdays or adjust days as needed (e.g., `BYDAY=MO,WE,FR` for Monday/Wednesday/Friday only).

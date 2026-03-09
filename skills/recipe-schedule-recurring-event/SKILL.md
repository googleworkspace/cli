---
name: recipe-schedule-recurring-event
version: 1.0.0
description: "Schedules a recurring Google Calendar event with attendees and verifies it was created. Use when a user wants to schedule a recurring meeting, set up a weekly standup, create a repeat event, add attendees to a calendar invite, book time on a shared calendar, or manage Google Calendar (gcal) entries. Handles creating the event with recurrence rules (e.g. RRULE weekly), setting timezones, and sending invites to attendees."
metadata:
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar"]
---

# Schedule a Recurring Meeting

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`

Create a recurring Google Calendar event with attendees.

## Steps

1. Create recurring event: `gws calendar events insert --params '{"calendarId": "primary"}' --json '{"summary": "Weekly Standup", "start": {"dateTime": "2024-03-18T09:00:00", "timeZone": "America/New_York"}, "end": {"dateTime": "2024-03-18T09:30:00", "timeZone": "America/New_York"}, "recurrence": ["RRULE:FREQ=WEEKLY;BYDAY=MO"], "attendees": [{"email": "team@company.com"}]}'`
   - Confirm the response includes an event ID before proceeding; if no ID is returned, treat this as a creation failure and follow the error handling steps below.
2. Verify the event was created: `gws calendar +agenda --days 14 --format table`
   - Confirm the event appears with the correct recurrence pattern (e.g. repeats weekly on Monday) and that all expected attendees are listed on the invite.

## Error Handling

- **Event creation fails:** Check that all attendee email addresses are valid, confirm the authenticated account has write access to the target calendar, and verify the `dateTime` format and timezone are correct.
- **Verification shows event missing:** Re-run the insert command and confirm the output returns an event ID. If the event still does not appear in the agenda, widen the `--days` window or check that the recurrence start date falls within the verification range.

---
name: recipe-reschedule-meeting
version: 1.0.0
description: "Move a Google Calendar event to a new time and automatically notify all attendees. Use when the user wants to reschedule a meeting, move a calendar event, change the time of an appointment, or shift a gcal event — including requests phrased as 'reschedule', 'move meeting', 'change time', or 'update appointment'."
metadata:
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar"]
---

# Reschedule a Google Calendar Meeting

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`

Move a Google Calendar event to a new time and automatically notify all attendees.

## Steps

1. Find the event: `gws calendar +agenda`
2. Get event details: `gws calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`
3. Update the time: `gws calendar events patch --params '{"calendarId": "primary", "eventId": "EVENT_ID", "sendUpdates": "all"}' --json '{"start": {"dateTime": "2025-01-22T14:00:00", "timeZone": "America/New_York"}, "end": {"dateTime": "2025-01-22T15:00:00", "timeZone": "America/New_York"}}'`
4. Verify the update: re-run the get command from Step 2 and confirm the new start/end times are reflected correctly.

## Error Handling

- **Event not found**: If Step 2 returns a 404, confirm the `eventId` from Step 1 is correct and that the event exists on the `primary` calendar.
- **Insufficient permissions**: If the patch returns a 403, ensure the authenticated account has write access to the calendar and is the event organiser or has been granted edit rights.

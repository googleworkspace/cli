---
name: recipe-share-event-materials
version: 1.0.0
description: "Shares Google Drive files with all attendees of a Google Calendar event. Use when asked to share, send, or distribute files/documents to meeting participants, event guests, calendar invitees, or everyone in a meeting — e.g. 'share this doc with all attendees', 'send the file to my meeting participants', 'distribute materials to event invitees', or 'share with everyone on the calendar invite'."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar", "gws-drive"]
---

# Share Files with Meeting Attendees

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`, `gws-drive`

Share Google Drive files with all attendees of a Google Calendar event.

## Steps

1. Get event attendees: `gws calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`
   - Extract the `attendees` array from the response; each entry contains an `email` field.
2. Share file with each attendee (repeat for every attendee email returned in step 1): `gws drive permissions create --params '{"fileId": "FILE_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "attendee@company.com"}'`
   - If sharing fails for a specific attendee (e.g., external user restrictions, permission denied), log the failure and continue with the remaining attendees. Report any failures in the final summary.
3. Verify sharing: `gws drive permissions list --params '{"fileId": "FILE_ID"}' --format table`
   - Confirm that all expected attendees appear in the permissions list. Flag any attendees missing from the list as unresolved sharing failures.

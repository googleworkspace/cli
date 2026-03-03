---
name: recipe-cancel-meeting
version: 1.0.0
description: "USE WHEN a meeting needs to be cancelled with notifications to all attendees."
metadata:
  openclaw:
    category: "recipe"
    domain: "meetings"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar", "gws-gmail"]
---

# Cancel a Meeting and Notify Attendees

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`, `gws-gmail`

USE WHEN a meeting needs to be cancelled with notifications to all attendees.

> [!CAUTION]
> The delete command will immediately cancel the event and notify attendees.

## Steps

1. Find the meeting: `gws calendar +agenda --format json` and locate the event ID
2. Delete the event (sends cancellation to attendees): `gws calendar events delete --params '{"calendarId": "primary", "eventId": "EVENT_ID", "sendUpdates": "all"}'`
3. Optionally send a follow-up explanation: `gws gmail +send --to attendees --subject 'Meeting Cancelled: [Title]'`


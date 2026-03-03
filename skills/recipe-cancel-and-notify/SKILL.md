---
name: recipe-cancel-and-notify
version: 1.0.0
description: "Delete a Google Calendar event and send a cancellation email via Gmail."
metadata:
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar", "gws-gmail"]
---

# Cancel Meeting and Notify Attendees

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`, `gws-gmail`

Delete a Google Calendar event and send a cancellation email via Gmail.

> [!CAUTION]
> Deleting with sendUpdates sends cancellation emails to all attendees.

## Steps

1. Find the meeting: `gws calendar +agenda --format json` and locate the event ID
2. Delete the event: `gws calendar events delete --params '{"calendarId": "primary", "eventId": "EVENT_ID", "sendUpdates": "all"}'`
3. Send follow-up: `gws gmail +send --to attendees --subject 'Meeting Cancelled: [Title]' --body 'Apologies, this meeting has been cancelled.'`


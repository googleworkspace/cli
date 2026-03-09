---
name: recipe-batch-invite-to-event
version: 1.0.0
description: "Adds a list of attendees (guests, participants) to an existing Google Calendar event and sends notifications. Use when a user wants to invite multiple people to a meeting, add guests or participants to a calendar event, send calendar invites via gcal, or bulk-add attendees to an existing event."
metadata:
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar"]
---

# Add Multiple Attendees to a Calendar Event

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`

Add a list of attendees to an existing Google Calendar event and send notifications.

## Steps

1. Get the event: `gws calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`
2. Add attendees: `gws calendar events patch --params '{"calendarId": "primary", "eventId": "EVENT_ID", "sendUpdates": "all"}' --json '{"attendees": [{"email": "alice@company.com"}, {"email": "bob@company.com"}, {"email": "carol@company.com"}]}'`
3. Verify attendees: `gws calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`

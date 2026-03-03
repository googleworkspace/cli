---
name: recipe-follow-up-email
version: 1.0.0
description: "USE WHEN the user needs to send follow-up notes and action items after a meeting."
metadata:
  openclaw:
    category: "recipe"
    domain: "communications"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail", "gws-calendar"]
---

# Send Follow-Up Emails After a Meeting

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`, `gws-calendar`

USE WHEN the user needs to send follow-up notes and action items after a meeting.

## Steps

1. Get meeting details: `gws workflow +meeting-prep` or `gws calendar events get --params '{"calendarId": "primary", "eventId": "EVENT_ID"}'`
2. Send follow-up to attendees: `gws gmail +send --to attendees@company.com --subject 'Follow-up: [Meeting Title]' --body 'Action items: ...'`
3. Create tasks from action items: `gws workflow +email-to-task --message-id MSG_ID`


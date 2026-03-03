---
name: recipe-recurring-meeting
version: 1.0.0
description: "USE WHEN the user needs to set up a recurring meeting (standup, weekly sync, etc.)."
metadata:
  openclaw:
    category: "recipe"
    domain: "meetings"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar"]
---

# Create a Recurring Team Meeting

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`

USE WHEN the user needs to set up a recurring meeting (standup, weekly sync, etc.).

> [!CAUTION]
> Confirm time zone and recurrence rule with the team before creating.

## Steps

1. Check existing schedule: `gws calendar +agenda --week --format table`
2. Create recurring event: `gws calendar events insert --params '{"calendarId": "primary"}' --json '{"summary": "Team Standup", "recurrence": ["RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"], "start": {"dateTime": "...", "timeZone": "..."}, "end": {"dateTime": "...", "timeZone": "..."}}'`


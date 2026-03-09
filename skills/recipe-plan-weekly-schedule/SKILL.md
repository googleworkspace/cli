---
name: recipe-plan-weekly-schedule
version: 1.0.0
description: "Reviews Google Calendar week, identifies scheduling gaps and free time, and adds events to fill them. Use when a user wants to plan their weekly schedule, review calendar availability, fill calendar gaps, book time blocks, or perform a weekly schedule review."
metadata:
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar"]
---

# Plan Your Weekly Google Calendar Schedule

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`

Reviews Google Calendar week, identifies gaps, and adds events to fill them.

## Steps

1. Check this week's agenda: `gws calendar +agenda`
2. Check free/busy for the week: `gws calendar freebusy query --json '{"timeMin": "2025-01-20T00:00:00Z", "timeMax": "2025-01-25T00:00:00Z", "items": [{"id": "primary"}]}'`
3. Review the freebusy response to confirm gaps exist before proceeding. If no gaps are found, report availability to the user and stop.
4. Add a new event: `gws calendar +insert --summary 'Deep Work Block' --start '2025-01-21T14:00' --duration 120`
   - If insertion fails due to a conflict, adjust the start time to an available slot identified in step 2 and retry.
5. Review updated schedule: `gws calendar +agenda`

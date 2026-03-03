---
name: recipe-schedule-interview
version: 1.0.0
description: "USE WHEN the user needs to schedule an interview with a candidate."
metadata:
  openclaw:
    category: "recipe"
    domain: "hiring"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar", "gws-gmail"]
---

# Schedule a Job Interview

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`, `gws-gmail`

USE WHEN the user needs to schedule an interview with a candidate.

> [!CAUTION]
> Confirm the interview time with the interviewer before sending the invitation.

## Steps

1. Check interviewer availability: `gws calendar +agenda --days 5 --format table`
2. Create the interview event: `gws calendar events insert --params '{"calendarId": "primary"}' --json '{"summary": "Interview: [Candidate]", "start": {"dateTime": "..."}, "end": {"dateTime": "..."}, "attendees": [{"email": "interviewer@company.com"}, {"email": "candidate@email.com"}]}'`
3. Send confirmation email: `gws gmail +send --to candidate@email.com --subject 'Interview Confirmation' --body 'Your interview has been scheduled...'`


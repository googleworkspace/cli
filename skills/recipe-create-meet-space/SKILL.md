---
name: recipe-create-meet-space
version: 1.0.0
description: "Creates a Google Meet meeting space and shares the join link via email. Use when the user asks to create a Google Meet, start a video call or video conference, generate a Meet link, set up a virtual meeting, or needs a Google video meeting link sent to participants."
metadata:
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins: ["gws"]
      skills: ["gws-meet", "gws-gmail"]
---

# Create a Google Meet Conference

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-meet`, `gws-gmail`

Create a Google Meet meeting space and share the join link.

## Steps

1. Create meeting space: `gws meet spaces create --json '{"config": {"accessType": "OPEN"}}'`
2. Verify the response contains a `meetingUri` field — if the field is absent or the command errors, stop and report the failure to the user before proceeding.
3. Copy the meeting URI from the response.
4. Email the link: `gws gmail +send --to team@company.com --subject 'Join the meeting' --body 'Join here: MEETING_URI'`
5. Confirm the send command succeeds — if it errors, report the failure and provide the meeting URI directly to the user so the link is not lost.

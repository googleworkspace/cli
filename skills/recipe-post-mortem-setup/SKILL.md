---
name: recipe-post-mortem-setup
version: 1.0.0
description: "Creates a Google Docs post-mortem document, schedules a Google Calendar review meeting, and sends a Google Chat notification — all in one coordinated workflow. Use when setting up an incident post-mortem, outage report, blameless retro, RCA (root cause analysis), or incident follow-up review across Google Workspace services. Trigger phrases: 'post-mortem', 'incident review', 'retrospective', 'retro', 'incident report', 'root cause analysis', 'blameless post-mortem'."
metadata:
  openclaw:
    category: "recipe"
    domain: "engineering"
    requires:
      bins: ["gws"]
      skills: ["gws-docs", "gws-calendar", "gws-chat"]
---

# Set Up Post-Mortem

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-docs`, `gws-calendar`, `gws-chat`

Create a Google Docs post-mortem, schedule a Google Calendar review, and notify via Chat.

## Steps

1. Create post-mortem doc: `gws docs +write --title 'Post-Mortem: [Incident]' --body '## Summary\n\n## Timeline\n\n## Root Cause\n\n## Action Items'`
   - Capture the returned doc URL (e.g. `DOC_URL`). If creation fails, stop and report the error before proceeding.

2. Schedule review meeting: `gws calendar +insert --summary 'Post-Mortem Review: [Incident]' --attendees team@company.com --start 'next monday 14:00' --duration 60 --description 'Post-mortem doc: [DOC_URL]'`
   - Replace `[DOC_URL]` with the URL captured in step 1. Verify the event was created successfully before proceeding.

3. Notify in Chat: `gws chat +send --space spaces/ENG_SPACE --text '🔍 Post-mortem scheduled for [Incident]. Doc: [DOC_URL]'`
   - Replace `[DOC_URL]` with the URL captured in step 1 so recipients can access the document directly from the notification.

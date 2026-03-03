---
name: recipe-post-mortem-setup
version: 1.0.0
description: "Create a Google Docs post-mortem, schedule a Google Calendar review, and notify via Chat."
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
2. Schedule review meeting: `gws calendar +insert --summary 'Post-Mortem Review: [Incident]' --attendees team@company.com --start 'next monday 14:00' --duration 60`
3. Notify in Chat: `gws chat +send --space spaces/ENG_SPACE --text '🔍 Post-mortem scheduled for [Incident].'`


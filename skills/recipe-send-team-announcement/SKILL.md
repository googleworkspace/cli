---
name: recipe-send-team-announcement
version: 1.0.0
description: "Sends a team announcement via both Gmail (email) and a Google Chat space (gchat/Hangouts Chat) in a single coordinated workflow. Use when the user wants to announce to the team, broadcast a message, notify everyone, or send to both email and chat simultaneously using Google Workspace tools."
metadata:
  openclaw:
    category: "recipe"
    domain: "communication"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail", "gws-chat"]
---

# Announce via Gmail and Google Chat

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`, `gws-chat`

Send a team announcement via both Gmail and a Google Chat space.

## Steps

1. Send email: `gws gmail +send --to team@company.com --subject 'Important Update' --body 'Please review the attached policy changes.'`
   - Confirm the email was sent successfully (look for a success confirmation or sent message ID) before proceeding. If the send fails, verify the recipient address and retry before moving to the next step.

2. Post in Chat: `gws chat +send --space spaces/TEAM_SPACE --text '📢 Important Update: Please check your email for policy changes.'`
   - Confirm the message was posted successfully (look for a message ID or delivery confirmation). If this step fails, verify the space ID is correct and that the account has access to the space.

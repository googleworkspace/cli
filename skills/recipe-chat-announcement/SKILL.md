---
name: recipe-chat-announcement
version: 1.0.0
description: "USE WHEN the user needs to post a message in a Google Chat space."
metadata:
  openclaw:
    category: "recipe"
    domain: "communications"
    requires:
      bins: ["gws"]
      skills: ["gws-chat"]
---

# Post an Announcement to a Chat Space

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-chat`

USE WHEN the user needs to post a message in a Google Chat space.

## Steps

1. List available spaces: `gws chat spaces list --params '{}'`
2. Send the message: `gws chat spaces messages create --params '{"parent": "spaces/SPACE_ID"}' --json '{"text": "📢 Announcement: ..."}'`


---
name: recipe-create-chat-space
version: 1.0.0
description: "USE WHEN the user needs to create a dedicated Chat space for a team or project."
metadata:
  openclaw:
    category: "recipe"
    domain: "communications"
    requires:
      bins: ["gws"]
      skills: ["gws-chat"]
---

# Create a New Chat Space for a Project

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-chat`

USE WHEN the user needs to create a dedicated Chat space for a team or project.

## Steps

1. Create the space: `gws chat spaces create --json '{"displayName": "Project Alpha", "spaceType": "SPACE"}'`
2. Add members: Invite team members to join the space
3. Post welcome message: `gws chat spaces messages create --params '{"parent": "spaces/SPACE_ID"}' --json '{"text": "Welcome to Project Alpha! 👋"}'`


---
name: recipe-create-task-from-email
version: 1.0.0
description: "USE WHEN the user wants to turn an email into an actionable task."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail", "gws-workflow"]
---

# Convert an Email to a Task

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`, `gws-workflow`

USE WHEN the user wants to turn an email into an actionable task.

## Steps

1. Find the email: `gws gmail +triage --max 5` to see recent messages
2. Get the message ID from the triage output
3. Convert to task: `gws workflow +email-to-task --message-id MSG_ID`
4. Verify the task was created: Check the output for the task ID and title


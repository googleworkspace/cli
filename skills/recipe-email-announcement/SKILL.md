---
name: recipe-email-announcement
version: 1.0.0
description: "USE WHEN the user needs to send an announcement email to a distribution list."
metadata:
  openclaw:
    category: "recipe"
    domain: "communications"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail"]
---

# Send a Company-Wide Announcement

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`

USE WHEN the user needs to send an announcement email to a distribution list.

> [!CAUTION]
> Company-wide emails reach everyone. Double-check recipients and content before sending.

## Steps

1. Draft the announcement: `gws gmail +send --to all-company@company.com --subject 'Important Update: [Topic]' --body '...'`


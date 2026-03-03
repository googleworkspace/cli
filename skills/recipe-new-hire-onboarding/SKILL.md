---
name: recipe-new-hire-onboarding
version: 1.0.0
description: "USE WHEN a new employee is joining and needs onboarding materials and calendar events."
metadata:
  openclaw:
    category: "recipe"
    domain: "onboarding"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar", "gws-drive", "gws-gmail", "gws-admin"]
---

# Set Up New Hire Onboarding

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`, `gws-drive`, `gws-gmail`, `gws-admin`

USE WHEN a new employee is joining and needs onboarding materials and calendar events.

> [!CAUTION]
> Verify the new hire's email address before creating the account.

## Steps

1. Create the user account: `gws admin users insert --json '{"primaryEmail": "newhire@company.com", "name": {"givenName": "First", "familyName": "Last"}, "password": "..."}' --params '{}'`
2. Schedule orientation sessions: `gws calendar events insert --params '{"calendarId": "primary"}' --json '{"summary": "Orientation: [New Hire]", ...}'`
3. Share onboarding docs folder: `gws drive permissions create --params '{"fileId": "FOLDER_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "newhire@company.com"}'`
4. Send welcome email: `gws gmail +send --to newhire@company.com --subject 'Welcome aboard!'`


---
name: recipe-onboarding-checklist
version: 1.0.0
description: "USE WHEN you need to track onboarding task completion in a spreadsheet."
metadata:
  openclaw:
    category: "recipe"
    domain: "onboarding"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets"]
---

# Track Onboarding Progress

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`

USE WHEN you need to track onboarding task completion in a spreadsheet.

## Steps

1. Find the onboarding tracker: `gws drive files list --params '{"q": "name contains '\''Onboarding Tracker'\''"}'`
2. Append a new row for the hire: `gws sheets +append --spreadsheet-id SHEET_ID --range 'Sheet1' --values '["New Hire Name", "Start Date", "Pending", "Pending", "Pending"]'`
3. Check current status: `gws sheets values get --params '{"spreadsheetId": "SHEET_ID", "range": "Sheet1"}'`


---
name: recipe-weekly-status-report
version: 1.0.0
description: "USE WHEN the user needs a summary of the week's activity."
metadata:
  openclaw:
    category: "recipe"
    domain: "reporting"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar", "gws-gmail", "gws-sheets"]
---

# Generate a Weekly Status Report

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`, `gws-gmail`, `gws-sheets`

USE WHEN the user needs a summary of the week's activity.

## Steps

1. Get the weekly digest: `gws workflow +weekly-digest --format json`
2. Get today's standup: `gws workflow +standup-report --format json`
3. Optionally log to a Sheet: `gws sheets +append --spreadsheet-id SHEET_ID --range 'Reports' --values '["Week of ...", meetings_count, unread_count]'`


---
name: recipe-create-events-from-sheet
version: 1.0.0
description: "Read event data from a Google Sheets spreadsheet and create Google Calendar entries for each row."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets", "gws-calendar"]
---

# Create Google Calendar Events from a Sheet

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`, `gws-calendar`

Read event data from a Google Sheets spreadsheet and create Google Calendar entries for each row.

## Steps

1. Read event data: `gws sheets +read --spreadsheet-id SHEET_ID --range "Events!A2:D"`
2. For each row, create a calendar event: `gws calendar +insert --summary 'Team Standup' --start '2025-01-20T09:00' --duration 30 --attendees alice@company.com,bob@company.com`


---
name: recipe-log-deal-update
version: 1.0.0
description: "Appends a deal status update to a Google Sheets sales tracking spreadsheet. Use when a user wants to log a deal update, track deal progress, add a sales update, update deal status, or record a CRM/pipeline entry in a spreadsheet — e.g. 'log a deal update', 'add to sales pipeline', 'record deal status', 'update the sales tracker', or 'track deal progress in Sheets'."
metadata:
  openclaw:
    category: "recipe"
    domain: "sales"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets", "gws-drive"]
---

# Log Deal Update to Sheet

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`, `gws-drive`

Append a deal status update to a Google Sheets sales tracking spreadsheet.

## Steps

1. Find the tracking sheet: `gws drive files list --params '{"q": "name = '\''Sales Pipeline'\'' and mimeType = '\''application/vnd.google-apps.spreadsheet'\''"}'`
   - If no results are returned, inform the user that no spreadsheet named "Sales Pipeline" was found and ask them to confirm the sheet name or share the spreadsheet ID directly.
2. Read current data: `gws sheets +read --spreadsheet-id SHEET_ID --range 'Pipeline!A1:F'`
3. Append new row: `gws sheets +append --spreadsheet-id SHEET_ID --range 'Pipeline' --values '["2024-03-15", "Acme Corp", "Proposal Sent", "$50,000", "Q2", "jdoe"]'`
4. Verify the append succeeded by reading the last row: `gws sheets +read --spreadsheet-id SHEET_ID --range 'Pipeline!A:F'` — confirm the newly appended row appears at the bottom of the data. If it is missing, report the failure to the user.

---
name: recipe-log-data-to-sheet
version: 1.0.0
description: "USE WHEN the user needs to append structured data to a spreadsheet."
metadata:
  openclaw:
    category: "recipe"
    domain: "reporting"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets"]
---

# Log Data to a Google Sheet

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`

USE WHEN the user needs to append structured data to a spreadsheet.

## Steps

1. Find the spreadsheet: `gws drive files list --params '{"q": "name = '\''My Tracker'\'' and mimeType = '\''application/vnd.google-apps.spreadsheet'\''"}'`
2. Read existing data: `gws sheets values get --params '{"spreadsheetId": "SHEET_ID", "range": "Sheet1"}'`
3. Append new rows: `gws sheets +append --spreadsheet-id SHEET_ID --range 'Sheet1' --values '["timestamp", "value1", "value2"]'`


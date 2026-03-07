---
name: "recipe-backup-sheet-as-csv"
version: "1.0.0"
description: "Export a Google Sheets spreadsheet as a CSV file for local backup or processing."
category: "recipe"
domain: "productivity"
requires:
  bins: ["gws"]
  skills: ["gws-sheets", "gws-drive"]
---
# Export a Google Sheet as CSV

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`, `gws-drive`

Export a Google Sheets spreadsheet as a CSV file for local backup or processing.

## Steps

1. Get spreadsheet details: `gws sheets spreadsheets get --params '{"spreadsheetId": "SHEET_ID"}'`
2. Export as CSV: `gws drive files export --params '{"fileId": "SHEET_ID", "mimeType": "text/csv"}'`
3. Or read values directly: `gws sheets +read --spreadsheet-id SHEET_ID --range 'Sheet1' --format csv`


---
name: recipe-copy-sheet-for-new-month
version: 1.0.0
description: "Duplicates a Google Sheets template tab for a new month of tracking. Use when a user wants to copy a sheet, duplicate a tab, create a new month spreadsheet, set up monthly tracking, or start a new month in Google Sheets — e.g. 'copy the template tab for February', 'create a new month sheet', 'duplicate my monthly tracking spreadsheet tab', or 'add a new month tab from template'."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets"]
---

# Copy a Google Sheet for a New Month

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`

Duplicate a Google Sheets template tab for a new month of tracking.

## Steps

1. Get spreadsheet details: `gws sheets spreadsheets get --params '{"spreadsheetId": "SHEET_ID"}'`
   - Inspect the returned `sheets` array to find the correct template sheet. Note its `sheetId` (the first sheet has `sheetId: 0` by default, but confirm this from the response rather than assuming).

2. Copy the template sheet: `gws sheets spreadsheets sheets copyTo --params '{"spreadsheetId": "SHEET_ID", "sheetId": 0}' --json '{"destinationSpreadsheetId": "SHEET_ID"}'`
   - Replace `sheetId: 0` with the actual `sheetId` of the template identified in step 1.

3. Validate the copy: `gws sheets spreadsheets get --params '{"spreadsheetId": "SHEET_ID"}'`
   - Confirm the new sheet appears in the `sheets` array and note its assigned `sheetId` before proceeding.

4. Rename the new tab: `gws sheets spreadsheets batchUpdate --params '{"spreadsheetId": "SHEET_ID"}' --json '{"requests": [{"updateSheetProperties": {"properties": {"sheetId": 123, "title": "February 2025"}, "fields": "title"}}]}'`
   - Replace `sheetId: 123` with the `sheetId` of the newly created sheet from step 3.

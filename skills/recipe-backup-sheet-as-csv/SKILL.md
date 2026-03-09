---
name: recipe-backup-sheet-as-csv
version: 1.0.0
description: "Export or download a Google Sheets spreadsheet as a CSV file for local backup or processing. Use when the user wants to export, download, convert, or save a Google Sheets spreadsheet as a .csv file, get local access to spreadsheet data, or extract a specific range from a Google Sheets document."
metadata:
  openclaw:
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
2. Export the full sheet as CSV via Drive: `gws drive files export --params '{"fileId": "SHEET_ID", "mimeType": "text/csv"}'`
   - Use this when you want to export the entire default sheet as a single CSV file.
3. Or read specific values directly via Sheets: `gws sheets +read --spreadsheet-id SHEET_ID --range 'Sheet1' --format csv`
   - Use this when you need a specific sheet tab or named range rather than the full file export.
4. Verify the export:
   - **Success:** The response body is non-empty, contains comma-separated values, and the first row matches the expected column headers from Step 1.
   - **Empty response:** Re-check that `SHEET_ID` is correct and the sheet tab is not empty; retry with the Sheets method (Step 3) to confirm data is readable.
   - **Unexpected format (e.g. HTML or error payload):** The file may not be a native Google Sheet — confirm the MIME type from Step 1 is `application/vnd.google-apps.spreadsheet` before exporting.

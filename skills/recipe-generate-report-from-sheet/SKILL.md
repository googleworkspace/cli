---
name: recipe-generate-report-from-sheet
version: 1.0.0
description: "Reads data from a Google Sheet (spreadsheet, .gsheet) and creates a formatted Google Docs report with summaries and structured content. Use when a user wants to generate a report from spreadsheet data, convert Sheets data into a Doc, create a Google Docs report from a Google Sheets source, or produce a formatted document from tabular data. Trigger phrases include: 'generate report from spreadsheet', 'convert sheet to document', 'create doc from Google Sheets data', 'Sheets to Docs report', 'export sheet as report'."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets", "gws-docs", "gws-drive"]
---

# Generate a Google Docs Report from Sheet Data

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`, `gws-docs`, `gws-drive`

Read data from a Google Sheet and create a formatted Google Docs report with summaries and structured sections.

## Steps

1. Read the data: `gws sheets +read --spreadsheet-id SHEET_ID --range 'Sales!A1:D'`
   - **Validate:** Confirm the response contains rows before proceeding. If the result is empty or an error is returned, stop and report that the sheet could not be read (check the spreadsheet ID and range, and ensure permissions are granted).

2. Create the report doc: `gws docs documents create --json '{"title": "Sales Report - January 2025"}'`
   - **Validate:** Confirm a `documentId` is returned in the response. Capture this as `DOC_ID` before proceeding. If creation fails (e.g., permission denied on Drive), stop and report the error.

3. Write the report: `gws docs +write --document-id DOC_ID --text '## Sales Report - January 2025

### Summary
Total deals: 45
Revenue: $125,000

### Top Deals
1. Acme Corp - $25,000
2. Widget Inc - $18,000'`
   - **Validate:** Confirm the write operation succeeds before sharing.

4. Share with stakeholders: `gws drive permissions create --params '{"fileId": "DOC_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "cfo@company.com"}'`

## Error Handling

- **Sheet not found / empty range:** Double-check `SHEET_ID` and the range string (e.g., `Sales!A1:D`). Ensure the authenticated account has at least Viewer access to the spreadsheet.
- **Permission denied on Doc creation or sharing:** Confirm the account has Drive write access and that the target email address is valid within the domain.
- **Missing DOC_ID:** If step 2 does not return a document ID, do not proceed to steps 3–4; report the failure and retry or escalate.

---
name: recipe-create-expense-tracker
version: 1.0.0
description: "Creates a Google Sheets spreadsheet for tracking expenses, including headers, expense categories, and initial entries. Use when the user asks to create an expense tracker, budget spreadsheet, or wants to set up financial tracking, money management, or expense logging in Google Sheets. Handles spreadsheet setup, row appending, and sharing with collaborators."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets", "gws-drive"]
---

# Create a Google Sheets Expense Tracker

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`, `gws-drive`

Creates a Google Sheets spreadsheet for tracking expenses with headers, expense categories, and initial entries. Covers spreadsheet creation, data entry, and sharing with collaborators.

## Steps

1. Create spreadsheet: `gws drive files create --json '{"name": "Expense Tracker 2025", "mimeType": "application/vnd.google-apps.spreadsheet"}'`
2. **Capture the returned `id` field as `SHEET_ID` from the response above before proceeding.** Verify the response contains a valid ID — if the creation failed or returned an error, do not continue.
3. Add headers: `gws sheets +append --spreadsheet-id SHEET_ID --range 'Sheet1' --values '["Date", "Category", "Description", "Amount"]'`
4. Add first entry: `gws sheets +append --spreadsheet-id SHEET_ID --range 'Sheet1' --values '["2025-01-15", "Travel", "Flight to NYC", "450.00"]'`
5. Share with manager: `gws drive permissions create --params '{"fileId": "SHEET_ID"}' --json '{"role": "reader", "type": "user", "emailAddress": "manager@company.com"}'`
6. Confirm success by checking that each command returned without an error status. If any step fails, report the error output to the user before attempting to continue or retry.

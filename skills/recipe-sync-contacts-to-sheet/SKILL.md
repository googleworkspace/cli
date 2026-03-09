---
name: recipe-sync-contacts-to-sheet
version: 1.0.0
description: "Exports Google Contacts directory to a Google Sheets spreadsheet, including names, email addresses, and phone numbers. Use when a user wants to export contacts, backup their address book, sync a contact list to a spreadsheet, or save their contact directory to Google Sheets."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-people", "gws-sheets"]
---

# Export Google Contacts to Sheets

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-people`, `gws-sheets`

Export Google Contacts directory to a Google Sheets spreadsheet, mapping names, email addresses, and phone numbers into rows.

## Steps

1. **List contacts** — retrieve up to 100 contacts from the domain directory:
   ```
   gws people people listDirectoryPeople --params '{"readMask": "names,emailAddresses,phoneNumbers", "sources": ["DIRECTORY_SOURCE_TYPE_DOMAIN_PROFILE"], "pageSize": 100}' --format json
   ```
   Parse the JSON response: for each entry, extract `names[0].displayName`, `emailAddresses[0].value`, and `phoneNumbers[0].value`. If the response includes a `nextPageToken`, repeat this step with `--params '{"pageToken": "<token>", ...}'` until no token is returned. Record the total number of contacts parsed across all pages.

2. **Write the header row** — create the sheet and add column headers:
   ```
   gws sheets +append --spreadsheet-id SHEET_ID --range 'Contacts' --values '["Name", "Email", "Phone"]'
   ```
   Verify the response confirms the header was written before proceeding.

3. **Append each contact row** — for every contact parsed in Step 1, append one row using the extracted values:
   ```
   gws sheets +append --spreadsheet-id SHEET_ID --range 'Contacts' --values '["<displayName>", "<emailAddress>", "<phoneNumber>"]'
   ```
   Replace `<displayName>`, `<emailAddress>`, and `<phoneNumber>` with the actual values from each contact. Use an empty string `""` for any field that is absent. If an append call returns an error or does not confirm a write, retry it once before moving on; note any contact rows that could not be written.

4. **Validate row count** — after all contacts have been appended, verify the total rows written matches the expected contact count from Step 1:
   ```
   gws sheets +get --spreadsheet-id SHEET_ID --range 'Contacts' --format json
   ```
   Compare the number of data rows returned (excluding the header) against the expected count. If the counts differ, identify which contacts are missing and retry appending them. Report the final result to the user, including any rows that could not be written after retrying.

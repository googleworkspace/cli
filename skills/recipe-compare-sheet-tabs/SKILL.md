---
name: recipe-compare-sheet-tabs
version: 1.0.0
description: "Reads data from two tabs in a Google Sheet to compare and identify differences such as missing rows, changed values, and added entries. Use when the user wants to compare spreadsheet tabs, diff two sheets, find discrepancies between Google Sheets tabs, detect changes between months or versions, or identify what rows or values were added, removed, or modified across two ranges in a Google Sheets file."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-sheets"]
---

# Compare Two Google Sheets Tabs

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-sheets`

Read data from two tabs in a Google Sheet to compare and identify differences such as missing rows, changed values, and added entries.

## Steps

1. Read the first tab: `gws sheets +read --spreadsheet-id SHEET_ID --range 'January!A1:D'`
   - **Validate:** Confirm the response contains data. If the result is empty or an error is returned, stop and inform the user — likely causes are an invalid spreadsheet ID or a non-existent tab name.

2. Read the second tab: `gws sheets +read --spreadsheet-id SHEET_ID --range 'February!A1:D'`
   - **Validate:** Confirm the response contains data. If either tab returns no rows (not even a header), flag it as an empty tab and halt comparison.

3. Compare the data and identify changes:
   - **Check headers first:** Confirm both tabs share the same column structure. If headers differ, flag the mismatch to the user before proceeding.
   - **Row-by-row comparison:** Match rows using a key column (e.g., the first column or a unique ID column). For each row, compare values across all columns.
   - **Classify each difference** into one of:
     - **Added** — row or value present in the second tab but not the first
     - **Removed** — row or value present in the first tab but not the second
     - **Changed** — row exists in both tabs but one or more column values differ
   - **Handle mismatched row counts:** If the tabs have different numbers of rows, note which tab has more rows and list the unmatched rows explicitly.

   **Comparison logic (Python pseudocode):**
   ```python
   headers = tab1[0]
   key_col = 0

   tab1_map = {row[key_col]: row for row in tab1[1:]}
   tab2_map = {row[key_col]: row for row in tab2[1:]}

   all_keys = set(tab1_map) | set(tab2_map)
   diffs = []

   for key in sorted(all_keys):
       if key not in tab1_map:
           diffs.append({"type": "Added", "key": key, ...})
       elif key not in tab2_map:
           diffs.append({"type": "Removed", "key": key, ...})
       else:
           for i, col in enumerate(headers):
               if tab1_map[key][i] != tab2_map[key][i]:
                   diffs.append({"type": "Changed", "key": key, "column": col,
                                 "tab1": tab1_map[key][i], "tab2": tab2_map[key][i]})
   ```

   - **Output format:** Present a structured summary, for example:

     | Type    | Key     | Column   | Tab 1 Value | Tab 2 Value |
     |---------|---------|----------|-------------|-------------|
     | Changed | Row 3   | Amount   | 100         | 150         |
     | Added   | Row 7   | —        | —           | New entry   |
     | Removed | Row 12  | —        | Old entry   | —           |

   - Conclude with a brief summary: total rows compared, number of additions, removals, and changes found.

## Error Handling

| Failure Case | Guidance |
|---|---|
| Invalid spreadsheet ID | Report the error immediately; do not attempt comparison. Ask the user to verify the ID. |
| Non-existent tab name | Confirm the exact tab name (case-sensitive) with the user and retry. |
| Empty tab | Flag which tab returned no data and halt; comparison requires at least a header row. |
| Header mismatch | Display both header rows side by side and ask the user how to proceed (abort or remap columns). |

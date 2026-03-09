---
name: recipe-create-events-from-sheet
version: 1.0.0
description: "Reads event data from a Google Sheets spreadsheet and bulk-creates Google Calendar entries for each row. Use when a user wants to import events from a spreadsheet, bulk create or schedule calendar appointments, sync a sheet to Google Calendar, or batch-add events from a .gsheet file. Trigger phrases: 'import events from spreadsheet', 'bulk create calendar events', 'create calendar events from sheet', 'schedule from sheet'."
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

Read event data from a Google Sheets spreadsheet and bulk-create Google Calendar entries for each row.

## Steps

### 1. Read Event Data from the Sheet

```
gws sheets +read --spreadsheet-id SHEET_ID --range 'Events!A2:D'
```

This returns a list of rows. Each row is expected to contain columns in this order:
- **A**: Event summary/title
- **B**: Start datetime (ISO 8601, e.g. `2025-01-20T09:00`)
- **C**: Duration in minutes
- **D**: Attendees (comma-separated emails)

**Example row:**

| A (Summary)  | B (Start)        | C (Duration) | D (Attendees)                     |
|--------------|------------------|--------------|-----------------------------------|
| Team Standup | 2025-01-20T09:00 | 30           | alice@company.com,bob@company.com |

### 2. Iterate Over Rows and Create Calendar Events

For each non-empty row returned from the sheet, extract the column values and call the calendar insert command. Use the following shell loop to process rows sequentially:

```bash
#!/usr/bin/env bash
# sheet_data.tsv: tab-separated output from the sheets read command (one row per line)
while IFS=$'\t' read -r summary start duration attendees; do
  # Skip rows missing required fields
  if [[ -z "$summary" || -z "$start" ]]; then
    echo "SKIPPED row: summary='$summary' start='$start'"
    continue
  fi

  gws calendar +insert \
    --summary "$summary" \
    --start "$start" \
    --duration "$duration" \
    --attendees "$attendees"

  echo "CREATED: $summary @ $start"
done < sheet_data.tsv
```

**Example for a single row:**
```
gws calendar +insert --summary 'Team Standup' --start '2025-01-20T09:00' --duration 30 --attendees alice@company.com,bob@company.com
```

Skip any row where required fields (summary, start) are empty or missing.

### 3. Validate Created Events

After processing all rows, verify that events were created successfully:

```
gws calendar +list --time-min '2025-01-20T00:00' --time-max '2025-01-21T00:00'
```

Check that the number of newly listed events matches the number of non-empty rows processed from the sheet.

## Error Handling

- **Invalid date format** — Ensure start datetime is ISO 8601 (e.g. `2025-01-20T09:00:00`); skip and report rows with unparseable dates.
- **Missing required fields** — Skip rows where `summary` or `start` is blank and log the row number.
- **API rate limits** — Pause briefly and retry the failed row before continuing.
- **Failed event creation** — Collect all failed rows and report them together at the end so the user can correct and re-run for only those rows.

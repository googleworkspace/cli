---
name: recipe-find-free-time
version: 1.0.0
description: "Queries Google Calendar free/busy status for multiple users to find a common meeting slot, then books the event. Use when scheduling meetings, checking availability, finding free time, or booking a meeting across multiple calendars. Handles calendar availability checks, overlapping schedule detection, and event creation via the gws-calendar skill."
metadata:
  openclaw:
    category: "recipe"
    domain: "scheduling"
    requires:
      bins: ["gws"]
      skills: ["gws-calendar"]
---

# Find Free Time Across Calendars

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-calendar`

Query Google Calendar free/busy status for multiple users to find a meeting slot.

## Steps

1. Query free/busy: `gws calendar freebusy query --json '{"timeMin": "2024-03-18T08:00:00Z", "timeMax": "2024-03-18T18:00:00Z", "items": [{"id": "user1@company.com"}, {"id": "user2@company.com"}]}'`
2. Interpret the freebusy response: The JSON output contains a `calendars` object keyed by user email. Each entry has a `busy` array of `{start, end}` time ranges. A slot is free for all users when it falls outside every user's busy ranges. Scan through the requested time window and identify gaps between busy periods that are long enough for the meeting. If no common free slot exists in the queried window, inform the user and suggest expanding the time range or querying a different day before proceeding.
3. Create event in the free slot: `gws calendar +insert --summary 'Meeting' --attendees user1@company.com,user2@company.com --start '2024-03-18T14:00:00' --duration 30`
4. Validate the booking: Inspect the JSON response from the insert command. Confirm the returned event object contains a valid `id` and that the `status` field is `"confirmed"`. If the command returns an error, handle common failures as follows:
   - **Calendar not found / permission denied**: Verify the attendee email addresses are correct and that the calendar is accessible.
   - **Conflict with existing event**: The chosen slot may have become occupied between the free/busy query and the insert; re-run step 1 to refresh availability and select a new slot.
   - **API / network error**: Retry the insert once; if the error persists, report the error details to the user.

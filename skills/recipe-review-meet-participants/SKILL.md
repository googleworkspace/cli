---
name: recipe-review-meet-participants
version: 1.0.0
description: "Reviews who attended a Google Meet conference, how long each participant stayed, and their join/leave session times. Use when asked to check meeting attendance, get a participant list, see who was on a call, review call duration, generate a meeting report, or look up attendees and their session details for a Google Meet conference."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-meet"]
---

# Review Google Meet Attendance

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-meet`

Review who attended a Google Meet conference and for how long.

## Steps

1. List recent conferences: `gws meet conferenceRecords list --format table`
   - If the list returns empty, there are no recorded conferences available. Confirm the correct Google Workspace account is active or try a different date range if filtering is supported.
   - Locate the `conferenceRecords/CONFERENCE_ID` value in the `name` column of the output for the target meeting.

2. List participants: `gws meet conferenceRecords participants list --params '{"parent": "conferenceRecords/CONFERENCE_ID"}' --format table`
   - Replace `CONFERENCE_ID` with the ID obtained in Step 1.
   - If the command returns empty, the conference record may not have participant data (e.g., the meeting had no recorded attendees). Verify the correct `CONFERENCE_ID` was used.
   - Locate the `PARTICIPANT_ID` value in the `name` column (e.g., `conferenceRecords/abc123/participants/456`) for each attendee of interest.

3. Get session details: `gws meet conferenceRecords participants participantSessions list --params '{"parent": "conferenceRecords/CONFERENCE_ID/participants/PARTICIPANT_ID"}' --format table`
   - Replace both `CONFERENCE_ID` and `PARTICIPANT_ID` with values from the previous steps.
   - Session details include join time, leave time, and duration for each session a participant had in the conference.
   - If no sessions are returned for a participant ID, confirm the ID was copied correctly from Step 2.

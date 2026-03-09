---
name: gws-meet
version: 1.0.0
description: "Creates and manages Google Meet video conferences, meeting spaces, participants, recordings, and transcripts via the gws CLI. Use when a user wants to schedule a virtual meeting, generate a Meet link, create or update a meeting space, list or retrieve conference records, manage meeting participants, or access Google Meet recordings and transcripts. Trigger terms: 'Google Meet', 'video call', 'meeting link', 'Meet link', 'virtual meeting', 'schedule a Meet', 'video conference', 'conference record'."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws meet --help"
---

# meet (v2)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws meet <resource> <method> [flags]
```

## API Resources

### conferenceRecords

  - `get` — Gets a conference record by conference ID.
  - `list` — Lists the conference records. By default, ordered by start time and in descending order.
  - `participants` — Operations on the 'participants' resource
  - `recordings` — Operations on the 'recordings' resource
  - `transcripts` — Operations on the 'transcripts' resource

### spaces

  - `create` — Creates a space.
  - `endActiveConference` — Ends an active conference (if there's one). For an example, see [End active conference](https://developers.google.com/workspace/meet/api/guides/meeting-spaces#end-active-conference).
  - `get` — Gets details about a meeting space. For an example, see [Get a meeting space](https://developers.google.com/workspace/meet/api/guides/meeting-spaces#get-meeting-space).
  - `patch` — Updates details about a meeting space. For an example, see [Update a meeting space](https://developers.google.com/workspace/meet/api/guides/meeting-spaces#update-meeting-space).

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws meet --help

# Inspect a method's required params, types, and defaults
gws schema meet.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Example Workflows

### Create a meeting space

1. Inspect the schema to identify available fields:
   ```bash
   gws schema meet.spaces.create
   ```
2. Create the space (no required params; optionally configure settings via `--json`):
   ```bash
   gws meet spaces create --json '{"config": {"accessType": "OPEN"}}'
   ```
   The response includes the `meetingUri` (the shareable Meet link) and the space `name` (e.g. `spaces/SPACE_ID`) for future operations.
3. Verify the space was created successfully by fetching it with the returned `name`:
   ```bash
   gws meet spaces get --params 'name=spaces/SPACE_ID'
   ```
   If the `get` call returns the space details, creation succeeded. If the create step returned an error (e.g. permission denied or invalid config), check auth setup in the shared skill and re-inspect the schema before retrying.

### List recent conference records

1. Inspect the schema for supported filters and pagination options:
   ```bash
   gws schema meet.conferenceRecords.list
   ```
2. List records, filtering by a specific space:
   ```bash
   gws meet conferenceRecords list --params 'filter=space.name="spaces/SPACE_ID"'
   ```
   Results are ordered by start time descending by default.
3. If the list is empty and records are expected, confirm the `SPACE_ID` is correct by running `gws meet spaces get --params 'name=spaces/SPACE_ID'` first. If the command itself fails, verify credentials and filters with `gws schema meet.conferenceRecords.list`.

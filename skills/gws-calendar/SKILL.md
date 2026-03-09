---
name: gws-calendar
version: 1.0.0
description: "Google Calendar CLI skill via the `gws` tool. Manages calendars and events: create events, check availability, schedule meetings, view upcoming appointments, set reminders, manage access control, and query free/busy time across calendars. Use when asked to schedule a meeting, book time, create a calendar invite, check gcal, view appointments, check availability, manage a calendar, or interact with Google Calendar in any way."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws calendar --help"
---

# calendar (v3)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws calendar <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+insert`](../gws-calendar-insert/SKILL.md) | create a new event |
| [`+agenda`](../gws-calendar-agenda/SKILL.md) | Show upcoming events across all calendars |

## API Resources

### acl

  - `delete` — Deletes an access control rule.
  - `get` — Returns an access control rule.
  - `insert` — Creates an access control rule.
  - `list` — Returns the rules in the access control list for the calendar.
  - `patch` — Updates an access control rule. This method supports patch semantics.
  - `update` — Updates an access control rule.
  - `watch` — Watch for changes to ACL resources.

### calendarList

  - `delete` — Removes a calendar from the user's calendar list.
  - `get` — Returns a calendar from the user's calendar list.
  - `insert` — Inserts an existing calendar into the user's calendar list.
  - `list` — Returns the calendars on the user's calendar list.
  - `patch` — Updates an existing calendar on the user's calendar list. This method supports patch semantics.
  - `update` — Updates an existing calendar on the user's calendar list.
  - `watch` — Watch for changes to CalendarList resources.

### calendars

  - `clear` — Clears a primary calendar. This operation deletes all events associated with the primary calendar of an account.
  - `delete` — Deletes a secondary calendar. Use calendars.clear for clearing all events on primary calendars.
  - `get` — Returns metadata for a calendar.
  - `insert` — Creates a secondary calendar.
The authenticated user for the request is made the data owner of the new calendar.

Note: We recommend to authenticate as the intended data owner of the calendar. You can use domain-wide delegation of authority to allow applications to act on behalf of a specific user. Don't use a service account for authentication. If you use a service account for authentication, the service account is the data owner, which can lead to unexpected behavior.
  - `patch` — Updates metadata for a calendar. This method supports patch semantics.
  - `update` — Updates metadata for a calendar.

### channels

  - `stop` — Stop watching resources through this channel

### colors

  - `get` — Returns the color definitions for calendars and events.

### events

  - `delete` — Deletes an event.
  - `get` — Returns an event based on its Google Calendar ID. To retrieve an event using its iCalendar ID, call the events.list method using the iCalUID parameter.
  - `import` — Imports an event. This operation is used to add a private copy of an existing event to a calendar. Only events with an eventType of default may be imported.
Deprecated behavior: If a non-default event is imported, its type will be changed to default and any event-type-specific properties it may have will be dropped.
  - `insert` — Creates an event.
  - `instances` — Returns instances of the specified recurring event.
  - `list` — Returns events on the specified calendar.
  - `move` — Moves an event to another calendar, i.e. changes an event's organizer. Note that only default events can be moved; birthday, focusTime, fromGmail, outOfOffice and workingLocation events cannot be moved.
  - `patch` — Updates an event. This method supports patch semantics.
  - `quickAdd` — Creates an event based on a simple text string.
  - `update` — Updates an event.
  - `watch` — Watch for changes to Events resources.

### freebusy

  - `query` — Returns free/busy information for a set of calendars.

### settings

  - `get` — Returns a single user setting.
  - `list` — Returns all user settings for the authenticated user.
  - `watch` — Watch for changes to Settings resources.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws calendar --help

# Inspect a method's required params, types, and defaults
gws schema calendar.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Example Workflows

### List upcoming events on the primary calendar

```bash
# 1. Inspect the method to learn required params and available flags
gws schema calendar.events.list

# 2. List events from now, ordered by start time
gws calendar events list \
  --params calendarId=primary \
  --params timeMin=$(date -u +%Y-%m-%dT%H:%M:%SZ) \
  --params orderBy=startTime \
  --params singleEvents=true \
  --params maxResults=10
```

### Create a new event

```bash
# 1. Inspect the insert method for required fields
gws schema calendar.events.insert

# 2. Create the event with a JSON body
gws calendar events insert \
  --params calendarId=primary \
  --json '{
    "summary": "Team standup",
    "start": {"dateTime": "2024-06-10T09:00:00-07:00"},
    "end":   {"dateTime": "2024-06-10T09:30:00-07:00"},
    "attendees": [{"email": "colleague@example.com"}]
  }'
```

### Query free/busy availability

```bash
# 1. Inspect the freebusy.query method
gws schema calendar.freebusy.query

# 2. Check availability for a calendar over a time window
gws calendar freebusy query \
  --json '{
    "timeMin": "2024-06-10T00:00:00Z",
    "timeMax": "2024-06-10T23:59:59Z",
    "items":   [{"id": "primary"}]
  }'
```

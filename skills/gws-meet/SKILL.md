---
name: gws-meet
description: "Manage Google Meet conferences."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws meet --help"
---

# meet (v2)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws meet <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+recent`](../gws-meet-recent/SKILL.md) | List recent meetings with title, code, duration and head count |
| [`+attendees`](../gws-meet-attendees/SKILL.md) | List who joined a meeting, when, and for how long |
| [`+transcript`](../gws-meet-transcript/SKILL.md) | Print a meeting transcript as speaker-labelled text |
| [`+notes`](../gws-meet-notes/SKILL.md) | Print Gemini "Take notes for me" notes for a meeting |
| [`+recording`](../gws-meet-recording/SKILL.md) | List a meeting's recordings and optionally download them |
| [`+new`](../gws-meet-new/SKILL.md) | Create a meeting link, optionally with a Calendar event |

## API Resources

### conferenceRecords

  - `get` — Gets a conference record by conference ID. For more information, see [Work with conferences](https://developers.google.com/workspace/meet/api/guides/conferences).
  - `list` — Lists the conference records. By default, ordered by start time and in descending order. For more information, see [Work with conferences](https://developers.google.com/workspace/meet/api/guides/conferences).
  - `participants` — Operations on the 'participants' resource
  - `recordings` — Operations on the 'recordings' resource
  - `smartNotes` — Operations on the 'smartNotes' resource
  - `transcripts` — Operations on the 'transcripts' resource

### spaces

  - `create` — Creates a space. For more information, see [Manage meeting spaces](https://developers.google.com/workspace/meet/api/guides/manage-meeting-spaces).
  - `endActiveConference` — Ends an active conference (if there's one). For more information, see [Manage meeting spaces](https://developers.google.com/workspace/meet/api/guides/manage-meeting-spaces).
  - `get` — Gets details about a meeting space. For more information, see [Manage meeting spaces](https://developers.google.com/workspace/meet/api/guides/manage-meeting-spaces). For an example, see [Get a meeting space](https://developers.google.com/workspace/meet/api/guides/meeting-spaces#get-meeting-space).
  - `patch` — Updates details about a meeting space. For more information, see [Manage meeting spaces](https://developers.google.com/workspace/meet/api/guides/manage-meeting-spaces).
  - `members` — Operations on the 'members' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws meet --help

# Inspect a method's required params, types, and defaults
gws schema meet.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


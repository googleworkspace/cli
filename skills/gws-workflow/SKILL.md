---
name: gws-workflow
version: 1.0.0
description: "Automates multi-step Google Workspace workflows spanning Gmail, Google Calendar, Drive, Docs, Sheets, and Tasks. Use when the user mentions Google Docs, Sheets, Gmail, Calendar, Drive, or Tasks, or asks to automate tasks across Google Workspace apps — such as generating a standup report from today's meetings, preparing for an upcoming meeting with agenda and linked docs, converting an email into a task, summarizing the week's activity, or announcing a Drive file in Google Chat."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws workflow --help"
---

# workflow (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws workflow <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+standup-report`](../gws-workflow-standup-report/SKILL.md) | Today's meetings + open tasks as a standup summary |
| [`+meeting-prep`](../gws-workflow-meeting-prep/SKILL.md) | Prepare for your next meeting: agenda, attendees, and linked docs |
| [`+email-to-task`](../gws-workflow-email-to-task/SKILL.md) | Convert a Gmail message into a Google Tasks entry |
| [`+weekly-digest`](../gws-workflow-weekly-digest/SKILL.md) | Weekly summary: this week's meetings + unread email count |
| [`+file-announce`](../gws-workflow-file-announce/SKILL.md) | Announce a Drive file in a Chat space |

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws workflow --help

# Inspect a method's required params, types, and defaults
gws schema workflow.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Example: Running a Workflow Command

```bash
# Convert a Gmail message into a Google Tasks entry
gws workflow email-to-task run \
  --params '{"messageId": "18d4f2a9c3b1e07f"}' \
  --json
```

## Validation for Cross-Service Workflows

Workflows that write data across multiple services (e.g., creating a Doc and emailing it via Gmail) can have cascading effects. After each step:

1. **Confirm** the preceding operation succeeded before triggering the next service call.
2. **Surface errors early** — if any step fails, report it and stop rather than continuing the chain.
3. **Verify side-effecting actions** (sending email, updating tasks, posting to Chat) with the user before executing when not explicitly pre-approved.

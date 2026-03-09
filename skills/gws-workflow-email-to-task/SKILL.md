---
name: gws-workflow-email-to-task
version: 1.0.0
description: "Google Workflow: Convert a Gmail message into a Google Tasks entry. Use when the user wants to create a task from an email, turn an email into a to-do, save a message as a task, convert inbox items to tasks, or use phrases like 'email to task', 'task from email', 'Gmail to-do', 'turn email into todo', or 'create task from message'."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws workflow +email-to-task --help"
---

# workflow +email-to-task

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Convert a Gmail message into a Google Tasks entry

## Usage

```bash
gws workflow +email-to-task --message-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | Gmail message ID to convert |
| `--tasklist` | — | @default | Task list ID (default: @default) |

## Examples

```bash
gws workflow +email-to-task --message-id MSG_ID
gws workflow +email-to-task --message-id MSG_ID --tasklist LIST_ID
```

## Confirmation Step

Before executing, always show the user what will be created and ask for confirmation:

```
I'll create the following task:
  Title: <email subject>
  Notes: <email snippet>
  List:  @default (or specified list)

Proceed? (yes/no)
```

Only run the command after the user confirms.

## Expected Output

A successful run creates a task and returns a summary such as:

```
✓ Task created
  Title: "Follow up on project proposal"
  Notes: "Hi, just wanted to circle back on the proposal we discussed…"
  List:  @default
  Task ID: abc123xyz
```

## Tips

- Reads the email subject as the task title and snippet as notes.
- Creates a new task — always confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-workflow](../gws-workflow/SKILL.md) — All cross-service productivity workflows commands

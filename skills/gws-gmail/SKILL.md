---
name: gws-gmail
version: 1.0.0
description: "Manages Gmail via the gws CLI — send emails, check inbox, search messages, organize with labels, delete messages, handle drafts, manage threads, and work with attachments. Use when asked to send an email, compose a message, write an email, check mail, read inbox, reply to email, search Gmail, list messages, manage labels, handle drafts, or perform any Gmail-related operation."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail --help"
---

# gmail (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws gmail <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+send`](../gws-gmail-send/SKILL.md) | Send an email |
| [`+triage`](../gws-gmail-triage/SKILL.md) | Show unread inbox summary (sender, subject, date) |
| [`+watch`](../gws-gmail-watch/SKILL.md) | Watch for new emails and stream them as NDJSON |

## API Resources

### users

  - `getProfile` — Gets the current user's Gmail profile.
  - `stop` — Stop receiving push notifications for the given user mailbox.
  - `watch` — Set up or update a push notification watch on the given user mailbox.
  - `drafts` — Operations on the 'drafts' resource
  - `history` — Operations on the 'history' resource
  - `labels` — Operations on the 'labels' resource
  - `messages` — Operations on the 'messages' resource
  - `settings` — Operations on the 'settings' resource
  - `threads` — Operations on the 'threads' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws gmail --help

# Inspect a method's required params, types, and defaults
gws schema gmail.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Examples

### Schema → Command Workflow

```bash
# 1. Inspect the method to learn required params and types
gws schema gmail.users.messages.list

# 2. Construct the command from schema output
gws gmail users messages list --params '{"userId":"me","q":"is:unread in:inbox","maxResults":10}'
```

### Common Operations

```bash
# List unread messages in inbox
gws gmail users messages list --params '{"userId":"me","q":"is:unread in:inbox","maxResults":10}'

# Get a specific message (full format includes headers and body)
gws gmail users messages get --params '{"userId":"me","id":"MESSAGE_ID","format":"full"}'
```

---
name: gws-gmail-reply
version: 1.0.0
description: "Gmail: Reply to a message (handles threading automatically)."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail +reply --help"
---

# gmail +reply

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Reply to a message (handles threading automatically)

## Usage

```bash
gws gmail +reply --message-id <ID> --body <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | Gmail message ID to reply to |
| `--body` | ✓ | — | Reply body (plain text) |
| `--from` | — | — | Sender address (for send-as/alias; omit to use account default) |
| `--cc` | — | — | Additional CC recipients (comma-separated) |
| `--dry-run` | — | — | Show the request that would be sent without executing it |

## Examples

```bash
gws gmail +reply --message-id 18f1a2b3c4d --body 'Thanks, got it!'
gws gmail +reply --message-id 18f1a2b3c4d --body 'Looping in Carol' --cc carol@example.com
```

## Tips

- Automatically sets In-Reply-To, References, and threadId headers.
- Quotes the original message in the reply body.
- For reply-all, use +reply-all instead.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

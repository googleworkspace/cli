---
name: gws-gmail-reply-all
version: 1.0.0
description: "Gmail: Reply-all to a message (handles threading automatically)."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail +reply-all --help"
---

# gmail +reply-all

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Reply-all to a message (handles threading automatically)

## Usage

```bash
gws gmail +reply-all --message-id <ID> --body <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message-id` | ✓ | — | Gmail message ID to reply to |
| `--body` | ✓ | — | Reply body (plain text) |
| `--from` | — | — | Sender address (for send-as/alias; omit to use account default) |
| `--cc` | — | — | Additional CC recipients (comma-separated) |
| `--remove` | — | — | Exclude recipients from the outgoing reply (comma-separated emails) |
| `--dry-run` | — | — | Show the request that would be sent without executing it |

## Examples

```bash
gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Sounds good to me!'
gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Updated' --remove bob@example.com
gws gmail +reply-all --message-id 18f1a2b3c4d --body 'Adding Eve' --cc eve@example.com
```

## Tips

- Replies to the sender and all original To/CC recipients.
- Use --remove to exclude recipients from the outgoing reply, including the sender or Reply-To target.
- The command fails if exclusions leave no reply target.
- Use --cc to add new recipients.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

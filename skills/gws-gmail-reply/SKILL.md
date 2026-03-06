---
name: gws-gmail-reply
version: 1.0.0
description: "Gmail: Reply to an email (threading handled automatically)."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail +reply --help"
---

# gmail +reply

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Reply to an email (threading handled automatically)

## Usage

```bash
gws gmail +reply --to <MSG_ID> --body <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--to` | ✓ | — | Message ID to reply to |
| `--body` | ✓ | — | Reply body (plain text) |
| `--all` | — | — | Reply to all recipients (reply-all) |

## Examples

```bash
gws gmail +reply --to 18e1a2b3c4d5e6f7 --body 'Thanks!'
gws gmail +reply --to 18e1a2b3c4d5e6f7 --body 'Sounds good' --all
```

## Tips

- Automatically sets threadId, In-Reply-To, and References headers.
- Quotes the original message body in the reply.
- Use --all to reply to all original recipients (reply-all).

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

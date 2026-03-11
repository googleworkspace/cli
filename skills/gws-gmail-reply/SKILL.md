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

| Flag           | Required | Default | Description                                                     |
| -------------- | -------- | ------- | --------------------------------------------------------------- |
| `--message-id` | ✓        | —       | Gmail message ID to reply to                                    |
| `--body`       | ✓        | —       | Reply body (plain text, or HTML if `--html` is set)             |
| `--from`       | —        | —       | Sender address (for send-as/alias; omit to use account default) |
| `--to`         | —        | —       | Additional To email address(es), comma-separated                |
| `--cc`         | —        | —       | Additional CC email address(es), comma-separated                |
| `--bcc`        | —        | —       | BCC email address(es), comma-separated                          |
| `--html`       | —        | —       | Treat `--body` as HTML content (default is plain text)          |
| `--dry-run`    | —        | —       | Show the request that would be sent without executing it        |

## Examples

```bash
gws gmail +reply --message-id 18f1a2b3c4d --body 'Thanks, got it!'
gws gmail +reply --message-id 18f1a2b3c4d --body 'Looping in Carol' --cc carol@example.com
gws gmail +reply --message-id 18f1a2b3c4d --body 'Adding Dave' --to dave@example.com
gws gmail +reply --message-id 18f1a2b3c4d --body 'Reply' --bcc secret@example.com
gws gmail +reply --message-id 18f1a2b3c4d --body '<b>Bold reply</b>' --html
```

## Tips

- Automatically sets In-Reply-To, References, and threadId headers.
- Quotes the original message in the reply body.
- With `--html`, the quoted block uses Gmail's `gmail_quote` CSS classes and preserves the original message's HTML formatting. Use HTML fragment tags (`<p>`, `<b>`, `<a>`, etc.) — no `<html>`/`<body>` wrapper needed.
- With `--html`, inline images embedded in the quoted message (`cid:` references) will appear broken. Externally hosted images are unaffected.
- `--to` adds extra recipients to the To field.
- For reply-all, use `+reply-all` instead.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

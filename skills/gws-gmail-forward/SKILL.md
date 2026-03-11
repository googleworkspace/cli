---
name: gws-gmail-forward
version: 1.0.0
description: "Gmail: Forward a message to new recipients."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail +forward --help"
---

# gmail +forward

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Forward a message to new recipients

## Usage

```bash
gws gmail +forward --message-id <ID> --to <EMAILS>
```

## Flags

| Flag           | Required | Default | Description                                                                   |
| -------------- | -------- | ------- | ----------------------------------------------------------------------------- |
| `--message-id` | ✓        | —       | Gmail message ID to forward                                                   |
| `--to`         | ✓        | —       | Recipient email address(es), comma-separated                                  |
| `--from`       | —        | —       | Sender address (for send-as/alias; omit to use account default)               |
| `--cc`         | —        | —       | CC email address(es), comma-separated                                         |
| `--bcc`        | —        | —       | BCC email address(es), comma-separated                                        |
| `--body`       | —        | —       | Optional note above the forwarded message (plain text, or HTML with `--html`) |
| `--html`       | —        | —       | Treat `--body` as HTML content (default is plain text)                        |
| `--dry-run`    | —        | —       | Show the request that would be sent without executing it                      |

## Examples

```bash
gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com
gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --body 'FYI see below'
gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --cc eve@example.com
gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --bcc secret@example.com
gws gmail +forward --message-id 18f1a2b3c4d --to dave@example.com --body '<p>FYI</p>' --html
```

## Tips

- Includes the original message with sender, date, subject, and recipients.
- With `--html`, the forwarded block uses Gmail's `gmail_quote` CSS classes and preserves the original message's HTML formatting. Use HTML fragment tags (`<p>`, `<b>`, `<a>`, etc.) — no `<html>`/`<body>` wrapper needed.
- With `--html`, inline images embedded in the forwarded message (`cid:` references) will appear broken. Externally hosted images are unaffected.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

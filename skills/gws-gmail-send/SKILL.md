---
name: gws-gmail-send
version: 1.0.0
description: "Gmail: Send an email."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail +send --help"
---

# gmail +send

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Send an email

## Usage

```bash
gws gmail +send --to <EMAIL> --subject <SUBJECT> --body <TEXT>
```

## Flags

| Flag        | Required | Default | Description                                              |
| ----------- | -------- | ------- | -------------------------------------------------------- |
| `--to`      | ✓        | —       | Recipient email address(es), comma-separated             |
| `--subject` | ✓        | —       | Email subject                                            |
| `--body`    | ✓        | —       | Email body (plain text)                                  |
| `--cc`      | —        | —       | CC email address(es), comma-separated                    |
| `--bcc`     | —        | —       | BCC email address(es), comma-separated                   |
| `--dry-run` | —        | —       | Show the request that would be sent without executing it |

## Examples

```bash
gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi Alice!'
gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --cc bob@example.com
gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi!' --bcc secret@example.com
```

## Tips

- Handles RFC 2822 formatting and base64 encoding automatically.
- For HTML bodies or attachments, use the raw API instead: `gws gmail users messages send --json '...'`

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

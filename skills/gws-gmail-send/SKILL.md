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

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--to` | ✓ | — | Recipient email address |
| `--subject` | ✓ | — | Email subject |
| `--body` | ✓ | — | Email body (plain text) |

## Examples

```bash
gws gmail +send --to alice@example.com --subject 'Hello' --body 'Hi Alice!'
```

## Tips

- Handles RFC 2822 formatting and base64 encoding automatically.
- For HTML bodies, attachments, or CC/BCC, use the raw API instead:
- gws gmail users messages send --json '...'

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

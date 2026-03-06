---
name: gws-gmail-forward
version: 1.0.0
description: "Gmail: Forward an email to new recipients."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail +forward --help"
---

# gmail +forward

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Forward an email to new recipients

## Usage

```bash
gws gmail +forward --message <MSG_ID> --to <EMAIL>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--message` | ✓ | — | Message ID to forward |
| `--to` | ✓ | — | Recipient email address |
| `--body` | — | — | Optional message to include above the forwarded content |

## Examples

```bash
gws gmail +forward --message 18e1a2b3c4d5e6f7 --to bob@example.com
gws gmail +forward --message 18e1a2b3c4d5e6f7 --to bob@example.com --body 'FYI'
```

## Tips

- Includes original message body and attachments.
- Adds standard forwarded-message attribution header.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

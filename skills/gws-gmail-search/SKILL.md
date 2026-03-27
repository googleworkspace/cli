---
name: gws-gmail-search
description: "Gmail: Search Gmail messages with full metadata."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws gmail +search --help"
---

# gmail +search

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Search Gmail messages with full metadata

## Usage

```bash
gws gmail +search --query <QUERY>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--query` | ✓ | — | Gmail search query (e.g., 'from:alice subject:report') |
| `--max` | — | 20 | Maximum messages to return (default: 20) |
| `--page-token` | — | — | Page token for continuing a previous search |

## Examples

```bash
gws gmail +search --query 'is:unread'
gws gmail +search --query 'from:boss subject:urgent' --max 5
gws gmail +search --query 'has:attachment' --format table
gws gmail +search --query 'newer_than:1d' --page-token <TOKEN>
```

## Tips

- Read-only — never modifies your mailbox.
- Defaults to JSON output. Labels are resolved to human-readable names.
- Use --page-token with the nextPageToken from a previous result to paginate.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

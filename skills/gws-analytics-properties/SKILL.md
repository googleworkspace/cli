---
name: gws-analytics-properties
description: "Google Analytics Data: List the Analytics accounts and GA4 properties you can access."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws analytics +properties --help"
---

# analytics +properties

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

List the Analytics accounts and GA4 properties you can access

## Usage

```bash
gws analytics +properties
```

## Examples

```bash
gws analytics +properties
gws analytics +properties --format json
```

## Tips

- Read-only. Use a property ID from here with: gws analytics +report <id>

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-analytics](../gws-analytics/SKILL.md) — All google analytics 4 reports and audience data commands

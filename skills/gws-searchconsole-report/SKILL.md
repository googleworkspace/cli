---
name: gws-searchconsole-report
description: "Google Search Console: Clicks, impressions, CTR and position for a site."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws searchconsole +report --help"
---

# searchconsole +report

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Clicks, impressions, CTR and position for a site

## Usage

```bash
gws searchconsole +report <site>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<site>` | ✓ | — | Property: sc-domain:example.com or https://www.example.com/ |
| `--days` | — | 28 | Days to report, ending today (default 28) |
| `--start` | — | — | Start date YYYY-MM-DD (overrides --days) |
| `--end` | — | — | End date YYYY-MM-DD (default today) |
| `--by` | — | query | Group by query, page, country, device, date or searchAppearance; comma-separate for several (default query) |
| `--limit` | — | 25 | Maximum rows (default 25, max 25000) |
| `--type` | — | web | Search type (default web) |
| `--final-only` | — | — | Only finalised data (by default the last 2-3 days of fresh, still-changing data are included) |

## Examples

```bash
gws searchconsole +report sc-domain:example.com
gws searchconsole +report sc-domain:example.com --by page --days 90 --limit 50
gws searchconsole +report https://www.example.com/ --by date --format csv
gws searchconsole +report sc-domain:example.com --by query,country --format json
```

## Tips

- Read-only. Default output is a table; --format csv|json|yaml also work.
- List your properties with: gws searchconsole sites list
- A newly added property can take a few days to show data.
- Needs the webmasters or webmasters.readonly scope.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-searchconsole](../gws-searchconsole/SKILL.md) — All search performance, sitemaps and url inspection commands

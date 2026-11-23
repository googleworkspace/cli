---
name: gws-analytics-report
description: "Google Analytics Data: GA4 audience report: users, sessions and views by a dimension."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws analytics +report --help"
---

# analytics +report

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

GA4 audience report: users, sessions and views by a dimension

## Usage

```bash
gws analytics +report <property>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<property>` | ✓ | — | GA4 property ID (123456789 or properties/123456789) |
| `--days` | — | 28 | Days to report, ending today (default 28) |
| `--start` | — | — | Start date YYYY-MM-DD (overrides --days) |
| `--end` | — | — | End date YYYY-MM-DD (default today) |
| `--by` | — | country | Dimension(s) to group by, comma-separated (default country). See --help for the list |
| `--metrics` | — | activeUsers,newUsers,sessions,screenPageViews,engagementRate | Comma-separated GA4 metrics (default activeUsers,newUsers,sessions,screenPageViews,engagementRate) |
| `--limit` | — | 25 | Maximum rows (default 25) |

## Examples

```bash
gws analytics +report 123456789
gws analytics +report 123456789 --by sessionSource --days 7
gws analytics +report 123456789 --by pagePath --metrics screenPageViews,averageSessionDuration
gws analytics +report 123456789 --by date --format csv
gws analytics +report 123456789 --by userAgeBracket,userGender
```

## Tips

- Read-only. Rows are sorted by the first metric, or by date when grouping by date.
- Find property IDs with: gws analytics +properties
- Age and gender need Google signals enabled, and GA4 hides small groups.
- Needs the analytics.readonly (or analytics) scope.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-analytics](../gws-analytics/SKILL.md) — All google analytics 4 reports and audience data commands

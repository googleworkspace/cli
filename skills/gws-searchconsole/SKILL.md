---
name: gws-searchconsole
description: "Google Search Console: Search performance, sitemaps and URL inspection."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws searchconsole --help"
---

# searchconsole (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws searchconsole <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+report`](../gws-searchconsole-report/SKILL.md) | Clicks, impressions, CTR and position for a site |

## API Resources

### searchanalytics

  - `query` — Query your data with filters and parameters that you define. Returns zero or more rows grouped by the row keys that you define. You must define a date range of one or more days. When date is one of the group by values, any days without data are omitted from the result list. If you need to know which days have data, issue a broad date range query grouped by date for any metric, and see which day rows are returned.

### sitemaps

  - `delete` — Deletes a sitemap from the Sitemaps report. Does not stop Google from crawling this sitemap or the URLs that were previously crawled in the deleted sitemap.
  - `get` — Retrieves information about a specific sitemap.
  - `list` — Lists the [sitemaps-entries](/webmaster-tools/v3/sitemaps) submitted for this site, or included in the sitemap index file (if `sitemapIndex` is specified in the request).
  - `submit` — Submits a sitemap for a site.

### sites

  - `add` — Adds a site to the set of the user's sites in Search Console.
  - `delete` — Removes a site from the set of the user's Search Console sites.
  - `get` — Retrieves information about specific site.
  - `list` — Lists the user's Search Console sites.

### urlInspection

  - `index` — Operations on the 'index' resource

### urlTestingTools

  - `mobileFriendlyTest` — Operations on the 'mobileFriendlyTest' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws searchconsole --help

# Inspect a method's required params, types, and defaults
gws schema searchconsole.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


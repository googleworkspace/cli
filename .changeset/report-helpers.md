---
"@googleworkspace/cli": minor
---

Add Search Console and Google Analytics report helpers: `gws searchconsole +report <site>` (clicks, impressions, CTR and position by query, page, country, device or date), `gws analytics +report <property>` (GA4 users, sessions and views by a dimension) and `gws analytics +properties`. Registers `searchconsole` and `analytics` (Analytics Data API) as services. Empty ranges print a clear "no data in range" line instead of an empty table.

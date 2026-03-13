---
"@googleworkspace/cli": minor
---

Use Google account timezone instead of machine-local time for day-boundary calculations in calendar and workflow helpers. Adds `--timezone` flag to `+agenda` for explicit override. Timezone is fetched from Calendar Settings API and cached for 24 hours.

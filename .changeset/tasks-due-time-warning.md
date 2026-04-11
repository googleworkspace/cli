---
"@googleworkspace/cli": patch
---

Warn when a Google Tasks request includes a non-midnight `due` time, since the API stores only the date and ignores time-of-day.

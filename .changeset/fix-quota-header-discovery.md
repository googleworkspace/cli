---
"@googleworkspace/cli": patch
---

Move x-goog-user-project header from default client headers to API request builder, fixing Discovery Document fetches failing with 403 when the quota project lacks certain APIs enabled

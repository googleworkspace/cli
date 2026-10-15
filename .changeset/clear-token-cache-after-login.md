---
"@googleworkspace/cli": patch
---

Clear cached OAuth access tokens after `gws auth login` so newly granted scopes take effect immediately instead of a 403 "insufficient authentication scopes" until the old cached token expires

---
"@googleworkspace/cli": patch
---

Make the advertised `<api>:<version>` syntax work for APIs that are not in the service registry (for example `gws searchconsole:v1 sites list` or `gws analyticsdata:v1beta properties runReport`). The API name and version are validated before use. Also applies to `gws schema` and `gws auth login --services`. Fixes #670.

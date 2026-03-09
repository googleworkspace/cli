---
"@googleworkspace/cli": patch
---

Fix `<api>:<version>` escape hatch for unlisted APIs.

Previously, `gws searchconsole:v1 sites list` would fail with "Unknown service" because
`parse_service_and_version` called `resolve_service(service_arg)?` which checks the
hardcoded service list before the version override could be applied. Now when an explicit
version is provided (via colon syntax or `--api-version`), the raw API name is passed through
to Discovery document fetching, which performs its own validation.

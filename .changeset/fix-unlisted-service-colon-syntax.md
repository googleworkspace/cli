---
"@googleworkspace/cli": patch
---

Fix `<api>:<version>` syntax for unlisted services. Previously, using the
colon syntax the CLI's own error message advertises (e.g. `chromepolicy:v1`)
still failed with "Unknown service" for any API not in the hardcoded services
list, even though the parsed version was silently discarded. The Discovery
fetch underneath already supports arbitrary service/version pairs; only the
CLI arg-resolution layer was blocking it.

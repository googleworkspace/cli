---
"@googleworkspace/cli": patch
---

Fix stdout/stderr contract: route human-readable empty-result messages and HTTP error bodies to stderr so that `gws ... | jq` pipe workflows receive valid JSON on stdout only

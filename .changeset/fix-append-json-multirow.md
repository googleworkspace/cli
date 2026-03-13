---
"@googleworkspace/cli": patch
---

Fix `+append --json-values` multi-row bug by changing AppendConfig.values to Vec<Vec<String>>

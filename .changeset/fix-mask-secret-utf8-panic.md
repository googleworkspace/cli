---
"@googleworkspace/cli": patch
---

Fix `mask_secret` panic on multi-byte UTF-8 secrets by using char-based indexing instead of byte-offset slicing

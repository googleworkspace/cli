---
"@googleworkspace/cli": patch
---

Fix stack overflow when resolving recursive schema references, preserving non-cyclic fields and leaving cyclic references intact.

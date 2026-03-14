---
"@googleworkspace/cli": patch
---

Extend input validation to reject dangerous Unicode characters (zero-width chars, bidi overrides, Unicode line/paragraph separators) that were not caught by the previous ASCII-range check

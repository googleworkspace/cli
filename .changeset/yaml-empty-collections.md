---
"@googleworkspace/cli": patch
---

Fix YAML mapping values containing empty arrays or objects by separating their
inline collection syntax from the mapping colon. This also fixes structured
Docs reader output with empty outlines, child tabs, or style maps.

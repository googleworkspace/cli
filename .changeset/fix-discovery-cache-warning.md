---
"@anthropic/gws": patch
---

Warn on stderr when discovery cache write fails

Previously, the error from writing the discovery document cache was
silently discarded (`let _ = e`). Now prints a warning to stderr so
users are aware their cache is not persisting (e.g., due to disk full
or permission issues).

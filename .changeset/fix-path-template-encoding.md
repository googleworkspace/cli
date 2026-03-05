---
"@googleworkspace/cli": patch
---

Fix URL template path expansion to safely encode path parameters, including
Sheets `range` values with Unicode and reserved characters. `{var}` expansions
now encode as a path segment, `{+var}` preserves slashes while encoding each
segment, and invalid path parameter/template mismatches fail fast.

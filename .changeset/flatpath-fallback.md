---
"@googleworkspace/cli": patch
---

Fix Slides presentations.get failure caused by flatPath placeholder mismatch

When a Discovery Document's `flatPath` uses placeholder names that don't match
the method's parameter names (e.g., `{presentationsId}` vs `presentationId`),
`build_url` now falls back to the `path` field which uses RFC 6570 operators
that resolve correctly.

Fixes #118

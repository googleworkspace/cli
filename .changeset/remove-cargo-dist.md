---
"@googleworkspace/cli": patch
---

Remove cargo-dist; use native Node.js fetch for npm binary installer

Replaces the cargo-dist generated release pipeline and npm package with:
- A custom GitHub Actions release workflow with matrix cross-compilation
- A zero-dependency npm installer using native `fetch()` (Node 18+)
- Removes axios, rimraf, detect-libc, console.table, and axios-proxy-builder dependencies from the published npm package

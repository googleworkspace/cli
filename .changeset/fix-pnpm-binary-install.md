---
"@googleworkspace/cli": patch
---

fix: auto-install binary on run if missing

pnpm skips postinstall when the package is already up to date.
This ensures run.js will auto-trigger install.js if the
binary is missing, fixing the 'gws binary not found' error.

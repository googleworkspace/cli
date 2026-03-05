---
"@googleworkspace/cli": patch
---

fix: resolve 401 errors for legacy credentials and improve auth export command

- Fixed `resolve_account()` rejecting legacy `credentials.enc` when no account registry exists, causing silent 401 errors on all commands
- Credential loading errors are now logged to stderr instead of silently discarded
- `gws auth export` now supports `--account EMAIL` for multi-account setups
- Documented `--unmasked` and `--account` flags in export help text

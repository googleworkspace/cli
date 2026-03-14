---
"@googleworkspace/cli": minor
---

feat(credential_store): default `keyring` backend no longer writes encryption key to disk

The `GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND` env var now supports three values:

- `keyring` (default): OS keyring only — the encryption key is never written to `~/.config/gws/.encryption_key`, giving the strongest security on platforms with a native keychain (macOS Keychain, Windows Credential Manager).
- `keyring-with-file`: OS keyring with `.encryption_key` file kept in sync as a durable backup (previous default behavior).
- `file`: file only, for Docker/CI/headless environments (unchanged).

Users who relied on the implicit file backup should set `GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND=keyring-with-file` to restore the previous behavior.

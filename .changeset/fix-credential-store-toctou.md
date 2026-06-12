---
"@googleworkspace/cli": patch
---

Fix silent auth wipe: the keyring backend in `resolve_key()` was auto-deleting the `.encryption_key` fallback file when a key was found in the OS keyring, contradicting the function's own docstring. The fix mirrors the keyring key to the file (via `save_key_file` + `ensure_key_dir` for atomic write with 0o600 permissions and 0o700 parent) instead of deleting it. Switching backends mid-session no longer wipes credentials.

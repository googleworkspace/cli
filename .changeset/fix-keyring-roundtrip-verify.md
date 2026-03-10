---
"@googleworkspace/cli": patch
---

fix(auth): verify keyring writes with round-trip read before trusting them

On macOS, `keyring::Entry::set_password()` can return `Ok(())` without
actually persisting to Keychain (phantom write). This caused the encryption
key to be lost between runs, breaking all commands with "Decryption failed".

The fix adds a `get_password()` verification after every `set_password()` and
always persists the key to the local file as a backup.

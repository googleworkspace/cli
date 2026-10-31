---
"@googleworkspace/cli": patch
---

Stop deleting `credentials.enc` and the token caches when they cannot be decrypted. The file is kept, other credential sources are still tried, and if none is found the error explains the decryption failure and points at `GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND` instead of reporting "No credentials found". Fixes #886.

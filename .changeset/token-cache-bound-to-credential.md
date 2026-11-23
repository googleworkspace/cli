---
"@googleworkspace/cli": patch
---

Stop serving a stale access token after the credentials change. `gws auth login` now clears the token caches, and cached tokens are bound to a fingerprint of the credential that minted them, so a re-login with new scopes, a login to another account, or switching `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` never reuses the previous credential's token.

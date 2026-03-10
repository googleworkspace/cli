---
"@googleworkspace/cli": minor
---

Add `GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND` env var for explicit keyring backend selection (`keyring` or `file`). Fixes credential key loss in Docker/keyring-less environments by never deleting `.encryption_key` and always persisting it as a fallback.

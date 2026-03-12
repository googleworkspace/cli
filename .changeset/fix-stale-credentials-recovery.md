---
"@googleworkspace/cli": patch
---

Auto-recover from stale encrypted credentials after upgrade: remove undecryptable `credentials.enc` and fall through to other credential sources (plaintext, ADC) instead of hard-erroring. Also sync encryption key file backup when keyring has key but file is missing.

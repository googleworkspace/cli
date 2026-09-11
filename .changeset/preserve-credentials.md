---
"@googleworkspace/cli": patch
---

Preserve saved encrypted credentials and token caches when credential loading,
decryption, or keyring access fails. Report recovery guidance and stop authentication
instead of silently selecting plaintext credentials or another account through ADC.
Explicit token and credentials-file overrides and intentional logout remain unchanged.

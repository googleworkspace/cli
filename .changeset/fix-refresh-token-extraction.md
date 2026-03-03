---
"@googleworkspace/cli": patch
---

Fix OAuth login failing with "no refresh token" error by decrypting the token cache before parsing and supporting the HashMap token format used by EncryptedTokenStorage

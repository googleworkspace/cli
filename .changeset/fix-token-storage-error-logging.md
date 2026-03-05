---
"@anthropic/gws": patch
---

Log token cache decryption/parse errors instead of silently swallowing

Previously, `load_from_disk` used four nested `if let Ok` blocks that
silently returned an empty map on any failure. When the encryption key
changed or the cache was corrupted, tokens silently stopped loading and
users were forced to re-authenticate with no explanation.

Now logs specific warnings to stderr for decryption failures, invalid
UTF-8, and JSON parse errors, with a hint to re-authenticate.

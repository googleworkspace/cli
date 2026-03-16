---
"@googleworkspace/cli": patch
---

Fix `+reply` failing with "Invalid To header" when the sender's display name contains commas or parentheses (e.g. `"Anderson, Rich (CORP)"`). Display names with RFC 2822 special characters are now re-quoted in `encode_address_header()`.

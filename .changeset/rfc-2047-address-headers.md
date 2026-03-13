---
"@googleworkspace/cli": patch
---

fix(gmail): RFC 2047 encode non-ASCII display names in To/From/Cc/Bcc headers

Fixes mojibake when sending emails to recipients with non-ASCII display names (e.g. Japanese, Spanish accented characters). The new `encode_address_header()` function parses mailbox lists, encodes only the display-name portion via RFC 2047 Base64, and leaves email addresses untouched.

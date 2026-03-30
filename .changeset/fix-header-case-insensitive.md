---
"@googleworkspace/cli": patch
---

fix(gmail): use case-insensitive matching for email headers in parse_message_headers

The Gmail API preserves original header casing from the sending MTA (e.g., Microsoft Exchange emits "CC" instead of "Cc"). Per RFC 5322, header field names are case-insensitive. This change normalizes header names to lowercase before matching, consistent with the existing `get_part_header` function in the same file.

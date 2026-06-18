---
"@googleworkspace/cli": patch
---

fix(gmail): match message header names case-insensitively.

`parse_message_headers` used exact-case string matching, so headers whose field
names use non-canonical casing — e.g. `"CC"` (common from Exchange/Outlook) or a
lowercase `"from"` from some MTAs — fell through and were silently dropped. This
dropped CC recipients from `+reply-all`. Per RFC 5322 §1.2.2 header field names
are case-insensitive (the sibling `get_part_header` already uses
`eq_ignore_ascii_case`). Normalize the name to lowercase before matching.

---
"@googleworkspace/cli": patch
---

Fix Gmail message header parsing to be case-insensitive (RFC 5322 §1.2.2). `+reply-all` no longer silently drops `Cc` recipients (or other headers) from Microsoft Exchange / Outlook senders that use non-canonical casing such as `CC`.

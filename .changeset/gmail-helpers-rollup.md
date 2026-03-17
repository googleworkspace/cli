---
"@googleworkspace/cli": minor
---

Gmail helpers rollup: mail-builder migration, --attachment flag, +read helper

- Migrate `+send`, `+reply`, `+reply-all`, and `+forward` to the `mail-builder` crate for RFC-compliant MIME construction
- Add `--from` flag to `+send` for send-as alias support
- Add `--attachment` flag to `+send` with MIME type auto-detection, multipart/mixed construction, and path traversal validation
- Add `+read` helper to extract message body and headers (text, HTML, or JSON output)
- RFC 2822 display name quoting is handled natively by `mail-builder`

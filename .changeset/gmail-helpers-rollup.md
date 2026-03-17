---
"@googleworkspace/cli": minor
---

Gmail helpers rollup: mail-builder migration, --attach flag (upload endpoint), +read helper

- Migrate `+send`, `+reply`, `+reply-all`, and `+forward` to the `mail-builder` crate for RFC-compliant MIME construction
- Add `--from` flag to `+send` for send-as alias support
- Add `-a`/`--attach` flag to all mail helpers (`+send`, `+reply`, `+reply-all`, `+forward`) with `mime_guess2` auto-detection, 25MB size validation, and upload endpoint support (35MB API limit vs 5MB metadata-only)
- Add `+read` helper to extract message body and headers (text, HTML, or JSON output)
- Make `OriginalMessage.thread_id` optional (`Option<String>`) for draft compatibility
- RFC 2822 display name quoting is handled natively by `mail-builder`
- Introduce `UploadSource` enum in executor for type-safe upload strategies

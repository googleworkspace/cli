---
"@googleworkspace/cli": patch
---

Fix HTTP 204 No Content responses being mis-routed to the binary-file download path.

Some endpoints (observed on `calendar.events.delete` and `drive.files.delete`) return an
empty 204 body that still carries a non-empty `Content-Type` header such as `text/html`.
Response routing decided JSON-vs-binary purely from that header, so these 204s fell
through to the binary handler, which created a zero-byte `download.html` in the current
working directory and printed a spurious "success" status — even for commands that never
took a `--output` flag and have nothing to do with downloading a file.

Response handling now classifies on status first: a 204 is always treated as having no
body, regardless of `Content-Type`, so no file is read or written for it.

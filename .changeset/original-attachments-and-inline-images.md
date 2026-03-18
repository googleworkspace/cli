---
"@googleworkspace/cli": minor
---

Forward original attachments by default and preserve inline images in HTML mode.

`+forward` now includes the original message's attachments and inline images by default,
matching Gmail web behavior. Use `--no-original-attachments` to opt out.
`+reply`/`+reply-all` with `--html` preserve inline images in the quoted body via
`multipart/related`. In plain-text mode, inline images are not included (matching Gmail web).

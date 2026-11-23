---
"@googleworkspace/cli": minor
---

Add YouTube helpers: `gws youtube +upload <video> --title <t>` uploads with a resumable session and progress, resumes after network errors, defaults to private, and can set a description (text or `@file`), tags, a custom thumbnail, a playlist and a scheduled publish time; `gws youtube +videos` lists the channel's recent uploads with views, likes and duration. Registers `youtube` (Data API v3) as a service.

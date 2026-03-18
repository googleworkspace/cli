---
"@googleworkspace/cli": minor
---

feat(gmail): auto-populate From header with display name from send-as settings

Fetch the user's send-as identities to set the From header with a display name in all mail helpers (+send, +reply, +reply-all, +forward), matching Gmail web client behavior. Also enriches bare `--from` emails with their configured display name.

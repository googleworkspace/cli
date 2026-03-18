---
"@googleworkspace/cli": patch
---

fix(gmail): handle reply-all to own message correctly

Reply-all to a message you sent no longer errors with "No To recipient remains." The original To recipients are now used as reply targets, matching Gmail web client behavior.

---
"@googleworkspace/cli": patch
---

fix(gmail): fix self-reply detection in +reply and +reply-all

Self-reply detection now uses all send-as identities (respecting Gmail's
treatAsAlias setting) instead of just the primary email and resolved alias.
Extended to +reply (was only in +reply-all). Identities are fetched once
and shared between sender resolution and self-reply detection.

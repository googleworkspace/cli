---
"gws": patch
---

Fix garbled non-ASCII email subjects in `gmail +send` by RFC 2047 encoding the Subject header and adding MIME-Version/Content-Type headers.

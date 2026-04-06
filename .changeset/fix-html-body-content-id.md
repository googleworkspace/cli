---
"@googleworkspace/cli": patch
---

fix(gmail): preserve HTML body when text/html part has Content-ID

Outlook/Exchange adds a Content-ID header to the text/html body part for
multipart/related referencing. The MIME walker incorrectly treated any part
with Content-ID as a non-body part, causing the HTML body to be silently
dropped. Replies to Outlook messages fell back to a plain-text conversion,
losing all formatting, nested blockquotes, and inline images.

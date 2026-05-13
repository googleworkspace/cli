---
name: recipe-save-email-attachments
description: "Find Gmail messages with attachments and save them to a Google Drive folder."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gws
      skills:
        - gws-gmail
        - gws-drive
---

# Save Gmail Attachments to Google Drive

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`, `gws-drive`

Find Gmail messages with attachments and save them to a Google Drive folder.

## Steps

1. Search for emails with attachments: `gws gmail users messages list --params '{"userId": "me", "q": "has:attachment from:client@example.com"}' --format table`
2. Get message details: `gws gmail users messages get --params '{"userId": "me", "id": "MESSAGE_ID"}'`
3. Download and decode attachment bytes:
   ```bash
   gws gmail users messages attachments get --params '{"userId": "me", "messageId": "MESSAGE_ID", "id": "ATTACHMENT_ID"}' \
     | jq -r '.data' \
     | python3 -c 'import base64,sys; d=sys.stdin.read().strip(); sys.stdout.buffer.write(base64.urlsafe_b64decode(d + "=" * (-len(d) % 4)))' \
     > attachment.pdf
   ```
4. Upload to Drive folder: `gws drive +upload ./attachment.pdf --parent FOLDER_ID`

---
name: recipe-save-email-attachments
version: 1.0.0
description: "Find Gmail messages with attachments and save them to a Google Drive folder. Use when the user wants to save email attachments, backup Gmail files, download inbox attachments, or transfer email files to cloud storage. Handles searching for messages, extracting attachment data, decoding it to a local file, and uploading to Drive."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-gmail", "gws-drive"]
---

# Save Gmail Attachments to Google Drive

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-gmail`, `gws-drive`

Find Gmail messages with attachments and save them to a Google Drive folder.

## Steps

1. Search for emails with attachments: `gws gmail users messages list --params '{"userId": "me", "q": "has:attachment from:client@example.com"}' --format table`
2. Get message details: `gws gmail users messages get --params '{"userId": "me", "id": "MESSAGE_ID"}'`
   - From the response, locate the `payload.parts` array. For each part where `filename` is non-empty, note the `filename` and the `body.attachmentId` value — this is the `ATTACHMENT_ID` to use in step 3. Repeat steps 3–5 for each attachment found.
3. Download attachment data: `gws gmail users messages attachments get --params '{"userId": "me", "messageId": "MESSAGE_ID", "id": "ATTACHMENT_ID"}'`
   - The response contains a `data` field with the file content encoded as base64url. Decode it and write the bytes to a local file (e.g., `./attachment.pdf`) using the `filename` captured in step 2.
4. Upload to Drive folder: `gws drive +upload --file ./attachment.pdf --parent FOLDER_ID`
5. Validate upload: confirm the file appears in the target folder by listing its contents: `gws drive files list --params '{"q": "'\''FOLDER_ID'\'' in parents"}' --format table`
   - Verify the uploaded filename is present. If it is missing, retry step 4 or check for errors before continuing.

> **Multiple attachments:** If a message has more than one attachment (multiple non-empty `filename` entries in `payload.parts`), repeat steps 3–5 for each `ATTACHMENT_ID`/filename pair before moving to the next message.

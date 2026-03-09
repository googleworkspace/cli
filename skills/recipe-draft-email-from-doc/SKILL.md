---
name: recipe-draft-email-from-doc
version: 1.0.0
description: "Reads content from a Google Doc and uses it as the body of a Gmail message. Use when a user wants to send a Google Doc as an email, email this document, use a Google Doc as an email body, draft or compose a Gmail from a Doc, or says things like 'send doc as email', 'email this document', 'compose message from Google Docs', or 'draft email from doc'. Handles reading document body text and passing it directly to Gmail as the email body."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-docs", "gws-gmail"]
---

# Draft a Gmail Message from a Google Doc

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-docs`, `gws-gmail`

Read content from a Google Doc and use it as the body of a Gmail message.

## Steps

1. Get the document content:
   ```
   gws docs documents get --params '{"documentId": "DOC_ID"}'
   ```

2. Extract the plain text from the response by reading the `body.content` array. Each element may contain a `paragraph` with `elements`, each of which has a `textRun.content` field. Concatenate all `textRun.content` values in order to reconstruct the full document text.

   Example response structure:
   ```json
   {
     "body": {
       "content": [
         {
           "paragraph": {
             "elements": [
               { "textRun": { "content": "Hello, this is the first paragraph.\n" } }
             ]
           }
         },
         {
           "paragraph": {
             "elements": [
               { "textRun": { "content": "This is the second paragraph.\n" } }
             ]
           }
         }
       ]
     }
   }
   ```
   Extracted body text: `"Hello, this is the first paragraph.\nThis is the second paragraph.\n"`

3. **Validate the extracted text** before sending. Present the reconstructed body to the user and confirm it contains the expected content. If the text appears incomplete, garbled, or empty, stop and report the extraction issue rather than proceeding. This step is important because sending an email is irreversible.

4. Send the email using the extracted text as the body:
   ```
   gws gmail +send --to recipient@example.com --subject 'Newsletter Update' --body 'EXTRACTED_TEXT'
   ```

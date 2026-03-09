---
name: recipe-create-feedback-form
version: 1.0.0
description: "Creates a Google Form survey or questionnaire for feedback and distributes it to recipients via Gmail. Use when the user wants to create a survey, poll, questionnaire, or feedback form using Google Forms, or needs to share and email a form link to attendees or respondents. Covers form creation with customizable titles, retrieving the responder URL, and sending the form via email distribution."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-forms", "gws-gmail"]
---

# Create and Share a Google Form

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-forms`, `gws-gmail`

Create a Google Form for feedback and share it via Gmail.

## Steps

1. Create form: `gws forms forms create --json '{"info": {"title": "Event Feedback", "documentTitle": "Event Feedback Form"}}'`
2. Verify the response contains both `formId` and `responderUri` fields before proceeding; if either is missing, do not continue to the email step.
3. Get the form URL from the response (`responderUri` field)
4. Email the form: `gws gmail +send --to attendees@company.com --subject 'Please share your feedback' --body 'Fill out the form: FORM_URL'`
5. Confirm the send command returns a success response (e.g., a message ID is present in the output); if it fails, report the error and retry or advise the user to check recipient addresses and Gmail permissions.

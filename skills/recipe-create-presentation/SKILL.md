---
name: recipe-create-presentation
version: 1.0.0
description: "Creates a new Google Slides presentation and adds initial slides, then shares it with team members. Use when the user asks to create Google Slides, make a slideshow, build a presentation deck, or start a new Google presentation. Triggers include: slides, deck, slideshow, Google Slides, Google presentation."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-slides"]
---

# Create a Google Slides Presentation

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-slides`

Create a new Google Slides presentation, add initial slides, and share it with team members.

## Steps

1. Create presentation: `gws slides presentations create --json '{"title": "Quarterly Review Q2"}'`
2. Extract the presentation ID from the response JSON — look for the `presentationId` field, e.g.:
   ```json
   { "presentationId": "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgVE2upms", "title": "Quarterly Review Q2" }
   ```
   Use `jq` to extract it if needed: `| jq -r '.presentationId'`
3. Verify the presentation was created successfully: `gws slides presentations get --presentationId PRESENTATION_ID`
4. Share with team: `gws drive permissions create --params '{"fileId": "PRESENTATION_ID"}' --json '{"role": "writer", "type": "user", "emailAddress": "team@company.com"}'`

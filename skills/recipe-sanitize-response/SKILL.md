---
name: recipe-sanitize-response
version: 1.0.0
description: "USE WHEN the user needs to sanitize content for PII or safety before processing."
metadata:
  openclaw:
    category: "recipe"
    domain: "security"
    requires:
      bins: ["gws"]
      skills: ["gws-modelarmor", "gws-gmail", "gws-drive"]
---

# Screen API Responses Through Model Armor

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-modelarmor`, `gws-gmail`, `gws-drive`

USE WHEN the user needs to sanitize content for PII or safety before processing.

## Steps

1. Create a sanitize template: See `gws modelarmor +create-template --help`
2. Use the template on API calls: Add `--sanitize TEMPLATE_NAME` to any gws command
3. Example: `gws gmail users messages get --params '{"userId": "me", "id": "MSG_ID"}' --sanitize my-template`


---
name: gws-forms
version: 1.0.0
description: "Manages Google Forms (surveys, questionnaires) via the gws CLI: create forms, add or modify questions, retrieve and analyse form responses, update publish settings, and manage response watches. Use when the user wants to create a Google Form or survey, build a questionnaire, fetch form responses, modify form settings or items, set up response notifications, or work with forms.google.com links. Trigger terms: 'Google Forms', 'survey', 'questionnaire', 'form responses', 'Google survey', 'create a form', 'gform'."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws forms --help"
---

# forms (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws forms <resource> <method> [flags]
```

## API Resources

### forms

  - `batchUpdate` — Change the form with a batch of updates.
  - `create` — Create a new form using the title given in the provided form message in the request. *Important:* Only the form.info.title and form.info.document_title fields are copied to the new form. All other fields including the form description, items and settings are disallowed. To create a new form and add items, you must first call forms.create to create an empty form with a title and (optional) document title, and then call forms.update to add the items.
  - `get` — Get a form.
  - `setPublishSettings` — Updates the publish settings of a form. Legacy forms aren't supported because they don't have the `publish_settings` field.
  - `responses` — Operations on the 'responses' resource
  - `watches` — Operations on the 'watches' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws forms --help

# Inspect a method's required params, types, and defaults
gws schema forms.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Workflows

### Two-Step Form Creation (Create + Add Items)

Creating a form with questions requires two calls: first create the empty form, then batch-add items.

**Step 1 — Create the empty form:**

```bash
gws forms forms create \
  --json '{"info": {"title": "Customer Satisfaction Survey", "document_title": "Customer Satisfaction Survey"}}'
# Note the returned `formId` for step 2.
```

**Step 1 validation — Confirm `formId` was returned before proceeding:**

If the response does not include a `formId`, do not proceed to step 2. Use `gws forms forms get` to verify the form exists:

```bash
gws forms forms get \
  --params 'formId=<formId-from-step-1>'
```

A successful response will include the form's `info` and `formId`. If the form is not found or the call errors, check for an invalid or missing `formId`, malformed JSON in step 1, or auth issues (see the shared SKILL.md).

**Step 2 — Add questions via batchUpdate:**

```bash
gws forms forms batchUpdate \
  --params 'formId=<formId-from-step-1>' \
  --json '{
    "requests": [
      {
        "createItem": {
          "item": {
            "title": "How satisfied are you?",
            "questionItem": {
              "question": {
                "required": true,
                "choiceQuestion": {
                  "type": "RADIO",
                  "options": [
                    {"value": "Very satisfied"},
                    {"value": "Satisfied"},
                    {"value": "Unsatisfied"}
                  ]
                }
              }
            }
          },
          "location": {"index": 0}
        }
      }
    ]
  }'
```

If `batchUpdate` fails, common causes are: an invalid or stale `formId`, malformed JSON in the request body, or missing required fields (e.g. `location`). Re-inspect the method schema with `gws schema forms.forms.batchUpdate` and verify the `formId` with `forms get` before retrying.

### Retrieve Form Responses

```bash
gws forms responses list \
  --params 'formId=<formId>'
```

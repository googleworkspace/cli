---
name: gws-forms
version: 1.0.0
description: "Read and write Google Forms."
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
  - `create` — Create a new form using the title given in the provided form message in the request. *Important:* Only the form.info.title and form.info.document_title fields are copied to the new form. All other fie
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


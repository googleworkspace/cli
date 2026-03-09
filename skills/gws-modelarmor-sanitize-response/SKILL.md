---
name: gws-modelarmor-sanitize-response
version: 1.0.0
description: "Google Model Armor: Sanitize and filter a model response through a Model Armor template for outbound content safety. Use when the user needs to filter AI-generated output, apply response moderation, run safety checks on model responses, clean up AI output, or detect harmful content using Google's Model Armor service. Supports text input or full JSON request body via a named template resource."
metadata:
  openclaw:
    category: "security"
    requires:
      bins: ["gws"]
    cliHelp: "gws modelarmor +sanitize-response --help"
---

# modelarmor +sanitize-response

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Sanitize a model response through a Model Armor template

## Usage

```bash
gws modelarmor +sanitize-response --template <NAME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--template` | ✓ | — | Full template resource name (projects/PROJECT/locations/LOCATION/templates/TEMPLATE) |
| `--text` | — | — | Text content to sanitize |
| `--json` | — | — | Full JSON request body (overrides --text) |

## Examples

```bash
gws modelarmor +sanitize-response --template projects/P/locations/L/templates/T --text 'model output'
model_cmd | gws modelarmor +sanitize-response --template ...
```

## Tips

- Use for outbound safety (model -> user).
- For inbound safety (user -> model), use +sanitize-prompt.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-modelarmor](../gws-modelarmor/SKILL.md) — All filter user-generated content for safety commands

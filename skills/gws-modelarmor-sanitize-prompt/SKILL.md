---
name: gws-modelarmor-sanitize-prompt
version: 1.0.0
description: "Google Model Armor: Sanitizes a user prompt through a Model Armor template, applying content filtering, safety checks, and policy enforcement to detect harmful or unsafe input before it reaches a model. Use when the user needs to validate prompts for safety, filter harmful or policy-violating content, perform input validation or harmful content detection, or apply Google Model Armor safety policies to user-generated text."
metadata:
  openclaw:
    category: "security"
    requires:
      bins: ["gws"]
    cliHelp: "gws modelarmor +sanitize-prompt --help"
---

# modelarmor +sanitize-prompt

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Sanitize a user prompt through a Model Armor template

## Usage

```bash
gws modelarmor +sanitize-prompt --template <NAME>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--template` | ✓ | — | Full template resource name (projects/PROJECT/locations/LOCATION/templates/TEMPLATE) |
| `--text` | — | — | Text content to sanitize |
| `--json` | — | — | Full JSON request body (overrides --text) |

## Examples

```bash
gws modelarmor +sanitize-prompt --template projects/P/locations/L/templates/T --text 'user input'
echo 'prompt' | gws modelarmor +sanitize-prompt --template ...
```

## Tips

- If neither --text nor --json is given, reads from stdin.
- For outbound safety, use +sanitize-response instead.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-modelarmor](../gws-modelarmor/SKILL.md) — All filter user-generated content for safety commands

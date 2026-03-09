---
name: gws-modelarmor-create-template
version: 1.0.0
description: "Google Model Armor: Creates a new Model Armor template that defines content filtering rules, safety thresholds, and blocked categories for AI model inputs and outputs on GCP. Use when the user wants to set up AI guardrails, configure content moderation policies, create a safety template, define input/output filtering rules, or establish GCP Model Armor security controls for prompt and response sanitisation."
metadata:
  openclaw:
    category: "security"
    requires:
      bins: ["gws"]
    cliHelp: "gws modelarmor +create-template --help"
---

# modelarmor +create-template

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Create a new Model Armor template

## Usage

```bash
gws modelarmor +create-template --project <PROJECT> --location <LOCATION> --template-id <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--project` | ✓ | — | GCP project ID |
| `--location` | ✓ | — | GCP location (e.g. us-central1) |
| `--template-id` | ✓ | — | Template ID to create |
| `--preset` | — | — | Use a preset template: jailbreak |
| `--json` | — | — | JSON body for the template configuration (overrides --preset) |

## Examples

```bash
gws modelarmor +create-template --project P --location us-central1 --template-id my-tmpl --preset jailbreak
gws modelarmor +create-template --project P --location us-central1 --template-id my-tmpl --json '{...}'
```

### Example `--json` body

```json
{
  "filterConfig": {
    "jailbreakFilter": {
      "filterEnforcement": "ENABLED"
    },
    "maliciousUriFilter": {
      "filterEnforcement": "ENABLED"
    }
  }
}
```

Pass a JSON body with `--json` to override the preset and define custom filter categories and enforcement levels.

## Tips

- Defaults to the jailbreak preset if neither --preset nor --json is given.
- The **jailbreak** preset enables protections against prompt-injection and prompt-jailbreak attempts that try to bypass model safety instructions.
- Use the resulting template name with +sanitize-prompt and +sanitize-response.
- After creation, verify the template exists with `gws modelarmor +get-template --project <PROJECT> --location <LOCATION> --template-id <ID>`.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-modelarmor](../gws-modelarmor/SKILL.md) — All filter user-generated content for safety commands

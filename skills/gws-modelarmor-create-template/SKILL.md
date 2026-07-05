---
name: gws-modelarmor-create-template
description: "Google Model Armor: Create a new Model Armor template."
metadata:
  version: 0.22.5
  openclaw:
    category: "security"
    requires:
      bins:
        - gws
    cliHelp: "gws modelarmor +create-template --help"
---

# modelarmor +create-template

> **REFERENCE:** See `../gws-shared/SKILL.md` for auth, global flags, and security rules. Treat it as background guidance; do not run auth, setup, cache, or `gws generate-skills` commands unless the user explicitly asks or a command fails because setup is missing.

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

## Tips

- Defaults to the jailbreak preset if neither --preset nor --json is given.
- Use the resulting template name with +sanitize-prompt and +sanitize-response.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-modelarmor](../gws-modelarmor/SKILL.md) — All filter user-generated content for safety commands

---
name: gws-apps-script-push
version: 1.0.0
description: "Upload local files to an Apps Script project"
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws apps-script +push --help"
---

# apps-script +push

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Upload local files to an Apps Script project

## Usage

```bash
gws apps-script +push --script <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--script` | ✓ | — | Script Project ID |
| `--dir` | — | — | Directory containing script files (defaults to current dir) |

## Examples

```bash
gws script +push --script SCRIPT_ID
gws script +push --script SCRIPT_ID --dir ./src
```

## Tips

- Supports .gs, .js, .html, and appsscript.json files.
- Skips hidden files and node_modules automatically.
- This replaces ALL files in the project.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-apps-script](../gws-apps-script/SKILL.md) — All manage and execute apps script projects commands

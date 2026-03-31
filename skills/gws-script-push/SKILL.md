---
name: gws-script-push
description: "Google Apps Script: Upload local files to an Apps Script project."
metadata:
  version: 0.22.4
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws script +push --help"
---

# script +push

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Upload local files to an Apps Script project

## Usage

```bash
gws script +push --script <ID>
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
- [gws-script](../gws-script/SKILL.md) — All manage google apps script projects commands

---
name: gws-drive-path
description: "Google Drive: Resolve a My Drive or shared-drive path to a file id."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws drive +path --help"
---

# drive +path

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Resolve a My Drive or shared-drive path to a file id

## Usage

```bash
gws drive +path <path>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<path>` | ✓ | — | Path such as 'Company/design/social' or 'Shared drives/<drive>/a/b' |
| `--create` | — | — | Create missing folders along the path |

## Examples

```bash
gws drive +path 'Company/design/social'
gws drive +path 'Shared drives/Engineering/specs/2026'
gws drive +path 'Reports/Q1\/Q2' --create
gws drive +path 'Archive/2026/posters' --create --dry-run
```

## Tips

- The path is resolved one folder name at a time. If two items in a folder
- share a name, the command lists their ids and stops; it never picks one.
- Paths start at My Drive ('My Drive/' is optional); 'Shared drives/<name>/'
- starts at a shared drive. Write a slash inside a name as '\/'.
- With --dry-run, --create reports the folders it would make.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-drive](../gws-drive/SKILL.md) — All manage files, folders, and shared drives commands

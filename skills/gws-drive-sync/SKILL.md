---
name: gws-drive-sync
description: "Google Drive: Make a Drive folder match a local directory."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws drive +sync --help"
---

# drive +sync

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Make a Drive folder match a local directory

## Usage

```bash
gws drive +sync <local> <folder>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<local>` | ✓ | — | Local directory (relative to the current directory) |
| `<folder>` | ✓ | — | Drive folder: id, URL or path (created if missing) |
| `--delete` | — | — | Move to the trash remote items that are not in the local directory |
| `--ids` | — | — | Write a JSON map of relative path -> Drive id to this file |
| `--exclude` | — | — | Skip local names (or relative paths, if the pattern has a '/') matching this glob; repeatable |

## Examples

```bash
gws drive +sync ./out 'Company/design/social/launch'
gws drive +sync ./out FOLDER_ID --dry-run
gws drive +sync ./posters 'Posters' --delete --ids drive.json
gws drive +sync ./site 'Site' --exclude '*.tmp' --exclude 'drafts/*'
```

## Tips

- New files are uploaded. Changed files (by size and MD5) are updated in
- place, so their ids and share links stay the same. Unchanged files are
- not touched. Subdirectories are synced recursively.
- --delete moves extra remote items to the trash (never deletes them
- permanently) and leaves Google Docs, Sheets and Slides alone.
- The whole plan is checked before anything changes: if two remote items
- share a name, or a file sits where a folder should be, the command lists
- every such path and changes nothing.
- .DS_Store, Thumbs.db, desktop.ini and .git are always skipped; symlinks
- are not followed.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-drive](../gws-drive/SKILL.md) — All manage files, folders, and shared drives commands

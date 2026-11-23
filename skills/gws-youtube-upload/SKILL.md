---
name: gws-youtube-upload
description: "Google YouTube Data API v3: Upload a video (resumable, with progress); private by default."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws youtube +upload --help"
---

# youtube +upload

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Upload a video (resumable, with progress); private by default

## Usage

```bash
gws youtube +upload <video> --title <TEXT>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `<video>` | ✓ | — | Path to the video file (under the current directory) |
| `--title` | ✓ | — | Video title (max 100 characters) |
| `--description` | — | — | Description text, or @path to read it from a file |
| `--tags` | — | — | Comma-separated tags |
| `--privacy` | — | private | private (default), unlisted or public |
| `--publish-at` | — | — | Schedule publication (RFC 3339, e.g. 2026-12-01T09:00:00+01:00); keeps the video private until then |
| `--category` | — | 22 | YouTube category ID (default 22, People & Blogs; 27 is Education) |
| `--thumbnail` | — | — | Custom thumbnail image (JPEG or PNG, max 2 MB) |
| `--playlist` | — | — | Add the video to this playlist ID (needs the youtube scope) |
| `--made-for-kids` | — | — | Declare the video as made for kids (default: not made for kids) |
| `--no-notify` | — | — | Don't notify subscribers about a public upload |

## Examples

```bash
gws youtube +upload film.mp4 --title 'MBPrep in 60 seconds'
gws youtube +upload film.mp4 --title 'Launch' --description @description.txt --tags mbprep,medicine
gws youtube +upload film.mp4 --title 'Launch' --privacy unlisted --thumbnail cover.jpg --playlist PLxxxx
gws youtube +upload film.mp4 --title 'Launch' --dry-run
```

## Tips

- Uploads are private unless you pass --privacy.
- The upload resumes after network errors; progress is shown on stderr.
- --dry-run prints the requests without reading credentials or uploading.
- Custom thumbnails need a verified channel. --playlist needs the youtube
- (or youtube.force-ssl) scope; youtube.upload alone can't add to playlists.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-youtube](../gws-youtube/SKILL.md) — All upload and manage youtube videos, playlists and channels commands

---
"@googleworkspace/cli": minor
---

Add Drive and Docs helpers: `drive +path` resolves a My Drive or shared-drive path to an id (refusing to guess between items that share a name; `--create` makes missing folders), `drive +sync` makes a Drive folder match a local directory (uploads new files, updates changed ones in place so ids and links stay, `--delete` trashes extras, `--ids` writes a path → id map), `docs +export` exports a Doc as Markdown, text or HTML with images as links, and `docs +write <file.md>` writes Markdown into a new or existing Doc as real formatting.

---
name: gws-sheets-read
version: 1.0.0
description: "Reads cell values and ranges from Google Sheets spreadsheets using the gws CLI. Use when the user asks to read, fetch, retrieve, or access data from Google Sheets, a gsheet, or a Google spreadsheet — including requests to get cell contents, sheet data, named ranges, or spreadsheet values from a specific range (e.g. 'Sheet1!A1:D10'). Covers common phrasings like 'read from Google Sheets', 'get spreadsheet values', 'fetch cell data', or 'pull data from a gsheet'."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws sheets +read --help"
---

# sheets +read

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Read values from a spreadsheet

## Usage

```bash
gws sheets +read --spreadsheet <ID> --range <RANGE>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--spreadsheet` | ✓ | — | Spreadsheet ID |
| `--range` | ✓ | — | Range to read (e.g. 'Sheet1!A1:B2') |

## Examples

```bash
gws sheets +read --spreadsheet ID --range 'Sheet1!A1:D10'
gws sheets +read --spreadsheet ID --range Sheet1
```

## Tips

- Read-only — never modifies the spreadsheet.
- For advanced options, use the raw values.get API.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-sheets](../gws-sheets/SKILL.md) — All read and write spreadsheets commands

---
name: gws-sheets-append
version: 1.0.0
description: "Google Sheets: Append one or more rows to a spreadsheet using the gws CLI. Use when the user wants to add a row, insert data, log entries, or append to a Google Sheet or Google spreadsheet (gsheet). Supports simple comma-separated values for single-row appends and JSON arrays for bulk multi-row inserts."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws sheets +append --help"
---

# sheets +append

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Append a row to a spreadsheet

## Usage

```bash
gws sheets +append --spreadsheet <ID>
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--spreadsheet` | ✓ | — | Spreadsheet ID |
| `--values` | — | — | Comma-separated values (simple strings) |
| `--json-values` | — | — | JSON array of rows, e.g. '[["a","b"],["c","d"]]' |

## Examples

```bash
gws sheets +append --spreadsheet ID --values 'Alice,100,true'
gws sheets +append --spreadsheet ID --json-values '[["a","b"],["c","d"]]'
```

## Tips

- Use --values for simple single-row appends.
- Use --json-values for bulk multi-row inserts.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## Verification

After running the append command, confirm success by checking:

1. **Exit code** — a zero exit code indicates the command completed without error; a non-zero exit code means the append failed.
2. **Read back the sheet** — use the relevant read command from [gws-sheets](../gws-sheets/SKILL.md) to retrieve the last rows and verify the new data appears as expected.
3. **Common errors:**
   - Auth/permission denied → re-check credentials per [gws-shared](../gws-shared/SKILL.md).
   - Invalid spreadsheet ID → confirm the ID is correct and the sheet is accessible.
   - Malformed `--json-values` → ensure the JSON array is valid and properly quoted for the shell.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-sheets](../gws-sheets/SKILL.md) — All read and write spreadsheets commands

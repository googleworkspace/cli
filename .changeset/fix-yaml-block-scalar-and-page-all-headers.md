---
"@googleworkspace/cli": patch
---

fix: YAML block scalar for strings with `#`/`:`, and repeated CSV/table headers with `--page-all`

**Bug 1 — YAML output: `drive#file` rendered as block scalar**

Strings containing `#` or `:` (e.g. `drive#file`, `https://…`) were
incorrectly emitted as YAML block scalars (`|`), producing output like:

```yaml
kind: |
  drive#file
```

Block scalars add an implicit trailing newline which changes the string
value and produces invalid-looking output.  The fix restricts block
scalar to strings that genuinely contain newlines; all other strings
are double-quoted, which is safe for any character sequence.

**Bug 2 — `--page-all` with `--format csv` / `--format table` repeats headers**

When paginating with `--page-all`, each page printed its own header row,
making the combined output unusable for downstream processing:

```
id,kind,name          ← page 1 header
1,drive#file,foo.txt
id,kind,name          ← page 2 header (unexpected!)
2,drive#file,bar.txt
```

Column headers (and the table separator line) are now emitted only for
the first page; continuation pages contain data rows only.

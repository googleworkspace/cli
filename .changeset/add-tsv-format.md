---
"@googleworkspace/cli": minor
---

Add `--format tsv` output format for tab-separated values

TSV is the standard format for shell pipeline tools (`cut -f2`, `awk -F'\t'`).
Supports the same features as `--format csv`: array-of-objects, array-of-arrays,
flat scalars, and `--page-all` pagination with header suppression on continuation
pages. Tab characters and newlines inside field values are replaced with spaces.

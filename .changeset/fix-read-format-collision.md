---
"@googleworkspace/cli": patch
---

Fix `+read` local `--format` arg colliding with the global `--format` flag.

Previously, `gmail +read` defined a local `--format` arg (values: `text`, `json`,
default: `text`). Because the global `--format` flag is declared with
`.global(true)`, clap propagated the `text` default up to the parent matches,
causing `main.rs` to emit a spurious `"unknown output format 'text'; falling back
to json"` warning on every `+read` invocation.

The local arg is renamed to `--body-format` to eliminate the name collision.
`--body-format text` (default) renders the decoded message body as plain text;
`--body-format json` returns the full structured JSON object, matching the previous
`--format json` behaviour.

**Migration note:** Scripts or aliases using `gws gmail +read --format json` must
be updated to `gws gmail +read --body-format json`.

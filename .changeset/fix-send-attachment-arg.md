---
"@googleworkspace/cli": patch
---

fix(gmail): remove dead `--attachment` arg from `+send`

The `+send` subcommand defined a duplicate `"attachment"` arg alongside the
`"attach"` arg already provided by `common_mail_args`. Since `parse_attachments`
reads `"attach"`, the `--attachment` flag was silently ignored. Removed the
dead duplicate.

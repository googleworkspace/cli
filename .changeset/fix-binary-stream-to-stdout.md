---
"@googleworkspace/cli": minor
---

Binary responses now stream to stdout when no `--output` flag is provided.

Previously, commands like `drive files export` and `drive files get --alt media`
always wrote content to a `download.{ext}` file in the current working directory
and printed a status JSON object to stdout, with no way to pipe the content
directly to another process.

Now, omitting `--output` streams the raw bytes to stdout (the Unix/curl default).
Use `--output <path>` to save the content to a named file as before.

The `mime_to_extension` helper, which existed only to generate the default
`download.{ext}` filename, has been removed as it is no longer needed.

**Migration note:** Scripts that read `download.txt` / `download.pdf` / etc.
from cwd after running `drive files export` must be updated to either redirect
stdout (`gws ... > file.txt`) or supply `--output file.txt` explicitly.

---
"@googleworkspace/cli": minor
---

Allow operators to set `GOOGLE_WORKSPACE_CLI_FILE_ROOT` to an existing directory
for `--output` and `--upload` paths while keeping CWD confinement by default.
Relative CLI paths remain CWD-relative. Reject invalid roots, parent traversal
with an explicit root, control characters, and symlink escapes, including
dangling symlinks. Directory flags retain their existing boundaries.

Reject canonical file paths that cannot be represented as UTF-8 at the CLI
string boundary, so explicit output/upload paths cannot silently become omitted
arguments.

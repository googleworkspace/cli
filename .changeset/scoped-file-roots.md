---
"@googleworkspace/cli": minor
---

Allow operators to set `GOOGLE_WORKSPACE_CLI_FILE_ROOT` to an existing directory
for `--output` and `--upload` paths while keeping CWD confinement by default.
Relative CLI paths remain CWD-relative. Reject invalid roots, parent traversal
with an explicit root, control characters, and symlink escapes, including
dangling symlinks. Directory flags retain their existing boundaries.

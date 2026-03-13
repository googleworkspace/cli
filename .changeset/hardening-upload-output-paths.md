---
"@googleworkspace/cli": patch
---

Harden --upload and --output path handling by validating file paths against traversal, absolute paths, control characters, and symlink escape before any file I/O.

---
"@googleworkspace/cli": minor
---

`gws auth login` now attempts to open the OAuth URL in the default browser automatically (`open` on macOS, `xdg-open` on Linux, `explorer` on Windows), falling back to the existing copy-paste flow when no opener is available.

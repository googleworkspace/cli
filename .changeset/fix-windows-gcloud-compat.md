---
"@googleworkspace/cli": patch
---

fix: use gcloud.cmd on Windows and show platform-correct config paths

On Windows, gcloud is installed as `gcloud.cmd` which Rust's `Command`
cannot find without the extension. Also replaced hardcoded `~/.config/gws/`
in error messages with the actual platform-resolved path.

---
"@googleworkspace/cli": patch
---

fix: drain stdout pipe to prevent project listing timeout during auth setup

Fixed `gws auth setup` timing out at step 3 (GCP project selection) for users
with many projects. The `gcloud projects list` stdout pipe was only read after
the child process exited, causing a deadlock when output exceeded the OS pipe
buffer (~64 KB). Stdout is now drained in a background thread to prevent the
pipe from filling up.

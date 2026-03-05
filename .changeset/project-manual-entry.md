---
"@googleworkspace/cli": patch
---

Add "Enter project ID manually" option to project picker in `gws auth setup`.

Users with large numbers of GCP projects often hit the 10-second listing timeout.
The picker now includes a "⌨ Enter project ID manually" item so users can type a
known project ID directly without waiting for `gcloud projects list` to complete.

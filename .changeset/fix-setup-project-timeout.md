---
"@googleworkspace/cli": patch
---

Fix project timeout and add manual entry in `gws auth setup` (closes #116)

Users who belong to organizations with thousands of GCP projects would hit
the hardcoded 10-second timeout in `gws auth setup` when the CLI attempted
to list their projects, blocking them from completing setup without manually
using `--project`.

This PR improves the experience for these users by:

1. Increasing the project listing timeout from 10s to 30s to accommodate larger lists.
2. Adding a new `✏️ Enter existing project ID` option to the interactive project picker.
   If the user's project is missing due to API limits or dropouts, they can now manually type
   the ID right from the setup menu.

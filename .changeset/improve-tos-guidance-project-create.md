---
"@googleworkspace/cli": patch
---

Improve `gws auth setup` project creation failures in step 3:
- Detect Google Cloud Terms of Service precondition failures and show actionable guidance (`gcloud auth list`, account verification, Console ToS URL).
- Detect invalid project ID format / already-in-use errors and show clearer guidance.
- In interactive setup, keep the wizard open and re-prompt for a new project ID instead of exiting immediately on create failures.

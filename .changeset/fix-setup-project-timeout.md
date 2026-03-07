---
"@googleworkspace/cli": patch
---

refactor: extract timeout constant and DRY project input logic in `gws auth setup`

- Extract `LIST_PROJECTS_TIMEOUT_SECS` constant (set to 30s) so the timeout value
  and its error message stay in sync automatically.
- Increase the project listing timeout from 10s to 30s to accommodate users with
  many GCP projects (addresses #116).
- Extract `prompt_project_id()` helper to deduplicate the TUI input logic shared
  between the "Create new project" and "Enter project ID manually" flows.

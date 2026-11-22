---
"@googleworkspace/cli": minor
---

Add named auth profiles (`--profile NAME` / `GOOGLE_WORKSPACE_CLI_PROFILE`) with isolated credentials, token caches and optional per-profile OAuth client config. Named profiles never fall back to gcloud's well-known Application Default Credentials, and invalid profile names are rejected instead of silently using the default profile.

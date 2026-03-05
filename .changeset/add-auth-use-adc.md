---
"gws": minor
---

feat(auth): add `gws auth use-adc` command for Application Default Credentials

Adds a new `gws auth use-adc` command that imports Application Default Credentials
(ADC) from `gcloud`, eliminating the need to create custom OAuth clients for team
setups.

**Usage:**
```bash
gcloud auth application-default login --project=my-project --scopes=<scopes>
gws auth use-adc
```

**Features:**
- Imports ADC credentials from gcloud to gws
- Includes `quota_project_id` support for proper API quota tracking
- Adds `x-goog-user-project` header to API requests when quota project is set
- Simplifies team onboarding (no custom OAuth client needed)

**Technical changes:**
- New `handle_use_adc()` function in `src/auth_commands.rs`
- Added `get_quota_project_id()` helper in `src/executor.rs`
- Request builder includes quota project header when available
- Supports both `~/.config/gcloud/` and `~/Library/Application Support/gcloud/` ADC paths

This makes team authentication simpler - everyone uses Google's built-in OAuth client
instead of creating/sharing custom clients.

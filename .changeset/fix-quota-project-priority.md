---
"@googleworkspace/cli": patch
---

Prioritize local project configuration and `GOOGLE_WORKSPACE_PROJECT_ID` over global Application Default Credentials (ADC) for quota attribution. This fixes 403 errors when the Drive API is disabled in a global gcloud project but enabled in the project configured for gws.

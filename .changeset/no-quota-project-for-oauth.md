---
"@googleworkspace/cli": patch
---

Stop sending `x-goog-user-project` derived from the OAuth client's `project_id`

The `project_id` in `client_secret.json` was used as the quota project on every request.
The API only honors that header when the authenticated end user holds
`serviceusage.services.use` on the project, so users who are not IAM members of it
received a 403 on every call. Quota for end-user OAuth credentials is already attributed
via the OAuth client ID.

The header is still sent for ADC credentials with a `quota_project_id`, and can be set
explicitly via `GOOGLE_WORKSPACE_PROJECT_ID`.

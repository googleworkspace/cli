---
"@googleworkspace/cli": patch
---

Exclude Workspace-admin-only scopes from the "Recommended" scope preset.

Scopes that require Google Workspace domain-admin access (`apps.*`,
`cloud-identity.*`, `ediscovery`, `directory.readonly`, `groups`) now return
`400 invalid_scope` when used by personal `@gmail.com` accounts. These scopes
are no longer included in the "Recommended" template, preventing login failures
for non-Workspace users.

Workspace admins can still select these scopes manually via the "Full Access"
template or by picking them individually in the scope picker.

Adds a new `is_workspace_admin_scope()` helper (mirroring the existing
`is_app_only_scope()`) that centralises this detection logic.

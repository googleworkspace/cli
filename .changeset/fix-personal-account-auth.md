---
"@googleworkspace/cli": patch
---

Fix `gws auth login` unusable with personal @gmail.com accounts (closes #119)

Two separate bugs compounded to make authentication impossible for personal users:

**Bug 1 — Workspace-admin scopes in "Recommended" preset**

The "Recommended" scope preset included admin-only scopes
(`apps.alerts`, `apps.groups.settings`, `cloud-identity.*`, `ediscovery`,
`directory.*`, `groups`, `chat.admin.*`, `classroom.*`) that Google rejects
with `400 invalid_scope` for personal `@gmail.com` accounts.

Added `is_workspace_admin_scope()` which filters these from the Recommended
preset and from the picker UI. They remain available via `--full` or `--scopes`
for Workspace domain admins who explicitly need them.

**Bug 2 — Custom `client_secret.json` silently ignored on macOS / Windows**

On macOS and Windows `dirs::config_dir()` resolves to a different path than
`~/.config/` (which the README documents for Linux). Users who followed the
README instructions on macOS placed the file at `~/.config/gws/client_secret.json`
but the CLI looked in `~/Library/Application Support/gws/` and silently failed.

`load_client_config()` now searches in this order:

1. `GOOGLE_WORKSPACE_CLI_CLIENT_SECRET_FILE` env var (explicit path override)
2. Platform-native config dir (existing behaviour)
3. `~/.config/gws/client_secret.json` XDG fallback (new — catches macOS/Windows users following Linux docs)

The error message now shows the actual OS-specific expected path.

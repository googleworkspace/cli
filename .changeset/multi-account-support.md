---
"gws": minor
---

### Multi-Account Support

Add support for managing multiple Google accounts with per-account credential storage.

**New features:**

- `--account EMAIL` global flag available on every command
- `GWS_ACCOUNT` environment variable as fallback
- `gws auth login --account EMAIL` — associates credentials with a specific account
- `gws auth list` — lists all registered accounts
- `gws auth default EMAIL` — sets the default account
- `gws auth logout --account EMAIL` — removes a specific account
- `login_hint` in OAuth URL for automatic account pre-selection in browser
- Email validation via Google userinfo endpoint after OAuth flow

**Breaking change:** Existing users must run `gws auth login` again after upgrading. The credential storage format has changed from a single `credentials.enc` to per-account files (`credentials.<b64-email>.enc`) with an `accounts.json` registry.

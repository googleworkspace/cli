---
"@googleworkspace/cli": patch
---

Fix auth failures when accounts.json registry is missing

Three related bugs caused all API calls to fail with "Access denied. No credentials provided" even after a successful `gws auth login`:

1. `resolve_account()` rejected valid `credentials.enc` as "legacy" when `accounts.json` was absent, instead of using them.
2. `main.rs` silently swallowed all auth errors, masking real failures behind a generic message.
3. `auth login` didn't include `openid`/`email` scopes, so `fetch_userinfo_email()` couldn't identify the user, causing credentials to be saved without an `accounts.json` entry.

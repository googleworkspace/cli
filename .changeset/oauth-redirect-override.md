---
"@googleworkspace/cli": minor
---

Support overriding the OAuth redirect target via `GOOGLE_WORKSPACE_CLI_OAUTH_REDIRECT_URI`, `GOOGLE_WORKSPACE_CLI_OAUTH_STATE`, and `GOOGLE_WORKSPACE_CLI_OAUTH_PORT`, so `gws auth login` can route through an external OAuth redirector (e.g. from a remote dev environment). Also percent-decode the authorization code from the callback so codes that pass through a redirector are exchanged correctly.

---
"@googleworkspace/cli": minor
---

Allow customizing the `gws auth login` OAuth callback so it works when the CLI runs on a remote host whose `localhost` the browser connot reach directly: `GOOGLE_WORKSPACE_CLI_OAUTH_REDIRECT_URI` overrides the redirect URI sent to Google, `GOOGLE_WORKSPACE_CLI_OAUTH_STATE` sets the `state` value passed to the OAuth server, and `GOOGLE_WORKSPACE_CLI_OAUTH_PORT` pins the local callback port. When a custom `state` is set, the value returned on the callback is verified against it to reject forged (CSRF) responses.

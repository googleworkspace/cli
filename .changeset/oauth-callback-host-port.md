---
"@googleworkspace/cli": minor
---

Add `--callback-port` flag to `gws auth login` so users can configure the OAuth callback server host and port. Both flags also read from environment variable `GOOGLE_WORKSPACE_CLI_CALLBACK_PORT` respectively (CLI flags take precedence). This is useful when the OAuth app is registered with a fixed redirect URI or when running in Docker/CI with port-forwarding.

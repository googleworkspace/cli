---
"@googleworkspace/cli": minor
---

Add `--callback-host` and `--callback-port` flags to `gws auth login` so users can configure the OAuth callback server host and port. Both flags also read from environment variables `GOOGLE_WORKSPACE_CLI_CALLBACK_HOST` and `GOOGLE_WORKSPACE_CLI_CALLBACK_PORT` respectively (CLI flags take precedence). This is useful when the OAuth app is registered with a fixed redirect URI or when running in Docker/CI with port-forwarding.

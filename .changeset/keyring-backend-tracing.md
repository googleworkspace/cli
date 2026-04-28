---
"@googleworkspace/cli": patch
---

Route the "Using keyring backend" log line through `tracing::info!` instead of raw `eprintln!`. The line now respects `GOOGLE_WORKSPACE_CLI_LOG` (so users can suppress it via `gws=warn` as the help text already advertises) and reaches the JSON log pipeline configured by `GOOGLE_WORKSPACE_CLI_LOG_FILE`.

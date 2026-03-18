---
"googleworkspace-cli": minor
---

Improve execution contract for agent reliability:

- **Semantic exit codes**: Errors now exit with typed codes (1=api, 2=auth, 3=validation, 4=discovery, 5=internal) instead of always exiting 1
- **`gws exit-codes`**: New command outputs machine-readable exit code taxonomy as JSON
- **Schema enrichment**: `gws schema <method>` now includes `timeout_ms` (30000) and `exit_codes` in output
- **SIGTERM handling**: `gws gmail +watch` and `gws events +subscribe` now handle SIGTERM for clean shutdown (in addition to Ctrl+C)
- **`--idempotency-key`**: New global flag sends an `Idempotency-Key` HTTP header on POST/PUT/PATCH requests

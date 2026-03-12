---
"@googleworkspace/cli": minor
---

Add structured exit codes for scriptable error handling

`gws` now exits with a type-specific code instead of always using `1`:

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | API error — Google returned a 4xx/5xx response |
| `2` | Auth error — credentials missing, expired, or invalid |
| `3` | Validation error — bad arguments, unknown service, invalid flag |
| `4` | Discovery error — could not fetch the API schema document |
| `5` | Internal error — unexpected failure |

Exit codes are documented in `gws --help` and in the README.

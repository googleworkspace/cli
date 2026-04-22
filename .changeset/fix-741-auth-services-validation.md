---
"@googleworkspace/cli": patch
---

`auth login -s` / `--services`: reject unknown service names at parse time with a clear error listing valid names. Previously, unrecognised tokens were silently dropped, causing a token to cover fewer services than the user intended.

**Behavior change:** `cloud-platform` is no longer injected automatically when a `--services` filter is active. It was previously added to every filtered login regardless of the services requested. It still appears when using `--full` (no filter) or the interactive discovery picker (no filter).

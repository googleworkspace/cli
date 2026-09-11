---
"@googleworkspace/cli": patch
---

Skip authentication for Discovery-generated API and `docs +write` dry-runs.
Validate and preview requests without accessing the keyring or reading, changing,
or deleting stored credentials and token caches. Dry-runs work offline with a
fresh cached Discovery schema; schema fetching on first use or cache expiry is
unchanged. Real requests retain their existing authentication and error handling.

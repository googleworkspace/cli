---
"@googleworkspace/cli": patch
---

feat(error): detect disabled APIs and guide users to enable them

When the Google API returns a 403 `accessNotConfigured` error (i.e., the
required API has not been enabled for the GCP project), `gws` now:

- Extracts the GCP Console enable URL from the error message body.
- Prints the original error JSON to stdout (machine-readable, unchanged shape
  except for an optional new `enable_url` field added to the error object).
- Prints a human-readable hint with the direct enable URL to stderr, along
  with instructions to retry after enabling.

This prevents a dead-end experience where users see a raw 403 JSON blob
with no guidance. The JSON output is backward-compatible; only an optional
`enable_url` field is added when the URL is parseable from the message.

Fixes #31

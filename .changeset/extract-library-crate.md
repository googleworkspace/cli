---
"@googleworkspace/cli": minor
---

Extract `google-workspace` library crate for programmatic Rust API access (closes #386)

Introduces a Cargo workspace with a new `google-workspace` library crate (`crates/google-workspace/`)
that exposes the core modules for use as a Rust dependency:

- `discovery` — Discovery Document types and fetching
- `error` — Structured `GwsError` type
- `services` — Service registry and resolution
- `validate` — Input validation and URL encoding
- `client` — HTTP client with retry logic

The `gws` binary crate re-exports all library types transparently — zero behavioral changes.

# Code Review Style Guide

## Project Architecture

`gws` is a Rust CLI that dynamically generates commands from Google Discovery Documents at runtime. It does NOT use generated Rust crates (`google-drive3`, etc.) for API interaction. Do not suggest adding API-specific crates to `Cargo.toml`.

## Security: Trusted vs Untrusted Inputs

This CLI is frequently invoked by AI/LLM agents. CLI arguments may be adversarial.

- **CLI arguments (untrusted)** — Must validate paths against traversal (`../../`), reject control characters, percent-encode URL path segments, and use `reqwest .query()` for query parameters. Validators: `validate_safe_output_dir()`, `validate_safe_dir_path()`, `encode_path_segment()`, `validate_resource_name()`.
- **Environment variables (trusted)** — Set by the user in their shell profile, `.env` file, or deployment config. Do NOT flag missing path validation on environment variable values. This is consistent with `XDG_CONFIG_HOME`, `CARGO_HOME`, etc.

## Test Coverage

The `codecov/patch` check requires new/modified lines to be covered by tests. Prefer extracting testable helper functions over embedding logic in `main`/`run`. Tests should cover both happy paths and rejection paths (e.g., pass `../../.ssh` and assert `Err`).

## Changesets

Every PR must include a `.changeset/<name>.md` file. Use `patch` for fixes/chores, `minor` for features, `major` for breaking changes.

## PR Scope

Review comments must stay within the PR's stated scope. If you spot an improvement opportunity that is unrelated to the PR's purpose (e.g., refactoring constants, adding support for a different credential type, making an unrelated function atomic), mark it as a **follow-up** suggestion — not a blocking review comment. Do not request changes that expand the PR beyond its original intent.

Examples of scope creep to avoid:
- A bug-fix PR should not grow into a refactoring PR.
- Adding constants for strings used elsewhere is a separate cleanup task.
- Making a pre-existing function atomic is an enhancement, not a fix for the current PR.

## Code Style

- Rust: `cargo clippy -- -D warnings` must pass. `cargo fmt` enforced via pre-commit hook.
- Node.js: Use `pnpm` not `npm`.
- OAuth scope strings in test code will trigger "restricted/sensitive scope" warnings — these are expected and should be ignored.

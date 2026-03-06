# AGENTS.md

## Project Overview

`gws` is a Rust CLI tool for interacting with Google Workspace APIs. It dynamically generates its command surface at runtime by parsing Google Discovery Service JSON documents.

> [!IMPORTANT]
> **Dynamic Discovery**: This project does NOT use generated Rust crates (e.g., `google-drive3`) for API interaction. Instead, it fetches the Discovery JSON at runtime and builds `clap` commands dynamically. When adding a new service, you only need to register it in `src/services.rs` and verify the Discovery URL pattern in `src/discovery.rs`. Do NOT add new crates to `Cargo.toml` for standard Google APIs.

> [!NOTE]
> **Package Manager**: Use `pnpm` instead of `npm` for Node.js package management in this repository.

## Build & Test

> [!IMPORTANT]
> **Test Coverage**: The `codecov/patch` check requires that new or modified lines are covered by tests. When adding code, extract testable helper functions rather than embedding logic in `main`/`run` where it's hard to unit-test. Run `cargo test` locally and verify new branches are exercised.

```bash
cargo build          # Build in dev mode
cargo clippy -- -D warnings  # Lint check
cargo test           # Run tests
```

## Changesets

Every PR must include a changeset file. Create one at `.changeset/<descriptive-name>.md`:

```markdown
---
"@googleworkspace/cli": patch
---

Brief description of the change
```

Use `patch` for fixes/chores, `minor` for new features, `major` for breaking changes. The CI policy check will fail without a changeset.

## Architecture

The CLI uses a **two-phase argument parsing** strategy:

1. Parse argv to extract the service name (e.g., `drive`)
2. Fetch the service's Discovery Document, build a dynamic `clap::Command` tree, then re-parse

### Source Layout

| File                      | Purpose                                                                                   |
| ------------------------- | ----------------------------------------------------------------------------------------- |
| `src/main.rs`             | Entrypoint, two-phase CLI parsing, method resolution                                      |
| `src/discovery.rs`        | Serde models for Discovery Document + fetch/cache                                         |
| `src/services.rs`         | Service alias → Discovery API name/version mapping                                        |
| `src/auth.rs`             | OAuth2 token acquisition via env vars, encrypted credentials, or ADC                      |
| `src/credential_store.rs` | AES-256-GCM encryption/decryption of credential files                                     |
| `src/auth_commands.rs`    | `gws auth` subcommands: `login`, `logout`, `setup`, `status`, `export`                    |
| `src/commands.rs`         | Recursive `clap::Command` builder from Discovery resources                                |
| `src/executor.rs`         | HTTP request construction, response handling, schema validation                           |
| `src/schema.rs`           | `gws schema` command — introspect API method schemas                                      |
| `src/error.rs`            | Structured JSON error output                                                              |

## Demo Videos

Demo recordings are generated with [VHS](https://github.com/charmbracelet/vhs) (`.tape` files).

```bash
vhs docs/demo.tape
```

### VHS quoting rules

- Use **double quotes** for simple strings: `Type "gws --help" Enter`
- Use **backtick quotes** when the typed text contains JSON with double quotes:
  ```
  Type `gws drive files list --params '{"pageSize":5}'` Enter
  ```
  `\"` escapes inside double-quoted `Type` strings are **not supported** by VHS and will cause parse errors.

### Scene art

ASCII art title cards live in `art/`. The `scripts/show-art.sh` helper clears the screen and cats the file. Portrait scenes use `scene*.txt`; landscape chapters use `long-*.txt`.

## Input Validation & URL Safety

> [!IMPORTANT]
> This CLI is frequently invoked by AI/LLM agents. Always assume inputs can be adversarial — validate paths against traversal (`../../.ssh`), restrict format strings to allowlists, reject control characters, and encode user values before embedding them in URLs.

> [!NOTE]
> **Environment variables are trusted inputs.** The validation rules above apply to **CLI arguments** that may be passed by untrusted AI agents. Environment variables (e.g. `GOOGLE_WORKSPACE_CLI_CONFIG_DIR`) are set by the user themselves — in their shell profile, `.env` file, or deployment config — and are not subject to path traversal validation. This is consistent with standard conventions like `XDG_CONFIG_HOME`, `CARGO_HOME`, etc.

### Path Safety (`src/validate.rs`)

When adding new helpers or CLI flags that accept file paths, **always validate** using the shared helpers:

| Scenario                               | Validator                                | Rejects                                                              |
| -------------------------------------- | ---------------------------------------- | -------------------------------------------------------------------- |
| File path for writing (`--output-dir`) | `validate::validate_safe_output_dir()`   | Absolute paths, `../` traversal, symlinks outside CWD, control chars |
| File path for reading (`--dir`)        | `validate::validate_safe_dir_path()`     | Absolute paths, `../` traversal, symlinks outside CWD, control chars |
| Enum/allowlist values (`--msg-format`) | clap `value_parser` (see `gmail/mod.rs`) | Any value not in the allowlist                                       |

```rust
// In your argument parser:
if let Some(output_dir) = matches.get_one::<String>("output-dir") {
    crate::validate::validate_safe_output_dir(output_dir)?;
    builder.output_dir(Some(output_dir.clone()));
}
```

### URL Encoding (`src/helpers/mod.rs`)

User-supplied values embedded in URL **path segments** must be percent-encoded. Use the shared helper:

```rust
// CORRECT — encodes slashes, spaces, and special characters
let url = format!(
    "https://www.googleapis.com/drive/v3/files/{}",
    crate::helpers::encode_path_segment(file_id),
);

// WRONG — raw user input in URL path
let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
```

For **query parameters**, use reqwest's `.query()` builder which handles encoding automatically:

```rust
// CORRECT — reqwest encodes query values
client.get(url).query(&[("q", user_query)]).send().await?;

// WRONG — manual string interpolation in query strings
let url = format!("{}?q={}", base_url, user_query);
```

### Resource Name Validation (`src/helpers/mod.rs`)

When a user-supplied string is used as a GCP resource identifier (project ID, topic name, space name, etc.) that gets embedded in a URL path, validate it first:

```rust
// Validates the string does not contain path traversal segments (`..`), control characters, or URL-breaking characters like `?` and `#`.
let project = crate::helpers::validate_resource_name(&project_id)?;
let url = format!("https://pubsub.googleapis.com/v1/projects/{}/topics/my-topic", project);
```

This prevents injection of query parameters, path traversal, or other malicious payloads through resource name arguments like `--project` or `--space`.

### Checklist for New Features

When adding a new helper or CLI command:

1. **File paths** → Use `validate_safe_output_dir` / `validate_safe_dir_path`
2. **Enum flags** → Constrain via clap `value_parser` or `validate_msg_format`
3. **URL path segments** → Use `encode_path_segment()`
4. **Query parameters** → Use reqwest `.query()` builder
5. **Resource names** (project IDs, space names, topic names) → Use `validate_resource_name()`
6. **Write tests** for both the happy path AND the rejection path (e.g., pass `../../.ssh` and assert `Err`)

## PR Labels

Use these labels to categorize pull requests and issues:

- `area: discovery` — Discovery document fetching, caching, parsing
- `area: http` — Request execution, URL building, response handling
- `area: docs` — README, contributing guides, documentation
- `area: tui` — Setup wizard, picker, input fields
- `area: distribution` — Nix flake, cargo-dist, npm packaging, install methods
- `area: mcp` — Model Context Protocol server/tools
- `area: auth` — OAuth, credentials, multi-account, ADC
- `area: skills` — AI skill generation and management

## Environment Variables

### Authentication

| Variable | Description |
|---|---|
| `GOOGLE_WORKSPACE_CLI_TOKEN` | Pre-obtained OAuth2 access token (highest priority; bypasses all credential file loading) |
| `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` | Path to OAuth credentials JSON (no default; if unset, falls back to credentials secured by the OS Keyring and encrypted in `~/.config/gws/`) |

| `GOOGLE_APPLICATION_CREDENTIALS` | Standard Google ADC path; used as fallback when no gws-specific credentials are configured |

### Configuration

| Variable | Description |
|---|---|
| `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` | Override the config directory (default: `~/.config/gws`) |

### OAuth Client

| Variable | Description |
|---|---|
| `GOOGLE_WORKSPACE_CLI_CLIENT_ID` | OAuth client ID (for `gws auth login` when no `client_secret.json` is saved) |
| `GOOGLE_WORKSPACE_CLI_CLIENT_SECRET` | OAuth client secret (paired with `CLIENT_ID` above) |

### Sanitization (Model Armor)

| Variable | Description |
|---|---|
| `GOOGLE_WORKSPACE_CLI_SANITIZE_TEMPLATE` | Default Model Armor template (overridden by `--sanitize` flag) |
| `GOOGLE_WORKSPACE_CLI_SANITIZE_MODE` | `warn` (default) or `block` |

### Helpers

| Variable | Description |
|---|---|
| `GOOGLE_WORKSPACE_PROJECT_ID` | GCP project ID fallback for `gmail watch` and `events subscribe` helpers (overridden by `--project` flag) |

All variables can also live in a `.env` file (loaded via `dotenvy`).

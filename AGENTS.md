# AGENTS.md

## Project Overview

`gws` is a Rust CLI tool for interacting with Google Workspace APIs. It dynamically generates its command surface at runtime by parsing Google Discovery Service JSON documents.

> [!IMPORTANT]
> **Dynamic Discovery**: This project does NOT use generated Rust crates (e.g., `google-drive3`) for API interaction. Instead, it fetches the Discovery JSON at runtime and builds `clap` commands dynamically. When adding a new service, you only need to register it in `src/services.rs` and verify the Discovery URL pattern in `src/discovery.rs`. Do NOT add new crates to `Cargo.toml` for standard Google APIs.

> [!NOTE]
> **Package Manager**: Use `pnpm` instead of `npm` for Node.js package management in this repository.

## Build & Test

```bash
cargo build          # Build in dev mode
cargo clippy -- -D warnings  # Lint check
cargo test           # Run tests
```

## Architecture

The CLI uses a **two-phase argument parsing** strategy:
1. Parse argv to extract the service name (e.g., `drive`)
2. Fetch the service's Discovery Document, build a dynamic `clap::Command` tree, then re-parse

### Source Layout

| File | Purpose |
|---|---|
| `src/main.rs` | Entrypoint, two-phase CLI parsing, method resolution |
| `src/discovery.rs` | Serde models for Discovery Document + fetch/cache |
| `src/services.rs` | Service alias → Discovery API name/version mapping |
| `src/auth.rs` | Headless OAuth2 via `yup-oauth2` |
| `src/commands.rs` | Recursive `clap::Command` builder from Discovery resources |
| `src/executor.rs` | HTTP request construction, response handling, schema validation |
| `src/schema.rs` | `gws schema` command — introspect API method schemas |
| `src/error.rs` | Structured JSON error output |

## Demo Videos

Demo recordings are generated with [VHS](https://github.com/charmbracelet/vhs) (`.tape` files).

```bash
vhs demo.tape        # YouTube Short (portrait 1080×1920)
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

## Environment Variables

- `GOOGLE_WORKSPACE_CLI_TOKEN` — Pre-obtained OAuth2 access token (highest priority; bypasses all credential file loading)
- `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` — Path to OAuth credentials JSON (no default; if unset, falls back to encrypted then plaintext credentials in `~/.config/gws/`)
- Supports `.env` files via `dotenvy`

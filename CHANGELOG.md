# @googleworkspace/cli

## 0.5.0

### Minor Changes

- 9cf6e0e: Add `--tool-mode compact|full` flag to `gws mcp`. Compact mode exposes one tool per service plus a `gws_discover` meta-tool, reducing context window usage from 200-400 tools to ~26.

### Patch Changes

- 0a16d0b: Add `-s`/`--services` flag to `gws auth login` to filter the scope picker
  by service name (e.g. `-s drive,gmail,sheets`). Also expands the workspace
  admin scope blocklist to include `chat.admin.*` and `classroom.*` patterns.
- 5205467: fix(setup): drain stale keypresses between TUI screen transitions

## 0.4.4

### Patch Changes

- e1e08eb: Fix highlight color on light terminal themes by using reverse video instead of a dark-gray background

## 0.4.3

### Patch Changes

- fc6bc95: Exclude Workspace-admin-only scopes from the "Recommended" scope preset.

  Scopes that require Google Workspace domain-admin access (`apps.*`,
  `cloud-identity.*`, `ediscovery`, `directory.readonly`, `groups`) now return
  `400 invalid_scope` when used by personal `@gmail.com` accounts. These scopes
  are no longer included in the "Recommended" template, preventing login failures
  for non-Workspace users.

  Workspace admins can still select these scopes manually via the "Full Access"
  template or by picking them individually in the scope picker.

  Adds a new `is_workspace_admin_scope()` helper (mirroring the existing
  `is_app_only_scope()`) that centralises this detection logic.

- 2aa6084: docs: Comprehensive README overhaul addressing user feedback.

  Added a Prerequisites section prior to the Quick Start to highlight the optional `gcloud` dependency.
  Expanded the Authentication section with a decision matrix to help users choose the correct authentication path.
  Added prominent warnings about OAuth "testing mode" limitations (the 25-scope cap) and the strict requirement to explicitly add the authorizing account as a "Test user" (#130).
  Added a dedicated Troubleshooting section detailing fixes for common OAuth consent errors, "Access blocked" issues, and `redirect_uri_mismatch` failures.
  Included shell escaping examples for Google Sheets A1 notation (`!`).
  Clarified the `npm` installation rationale and added explicit links to pre-built native binaries on GitHub Releases.

## 0.4.2

### Patch Changes

- d3e90e4: fix: use ~/.config/gws on all platforms for consistent config path

  Previously used `dirs::config_dir()` which resolves to different paths per OS
  (e.g. ~/Library/Application Support/gws on macOS, %APPDATA%\gws on Windows),
  contradicting the documented ~/.config/gws/ path. Now uses ~/.config/gws/
  everywhere with a fallback to the legacy OS-specific path for existing installs.

## 0.4.1

### Patch Changes

- dbda001: Add "Enter project ID manually" option to project picker in `gws auth setup`.

  Users with large numbers of GCP projects often hit the 10-second listing timeout.
  The picker now includes a "⌨ Enter project ID manually" item so users can type a
  known project ID directly without waiting for `gcloud projects list` to complete.

## 0.4.0

### Minor Changes

- 87e4bb1: Add Linux ARM64 build targets (aarch64-unknown-linux-gnu and aarch64-unknown-linux-musl) to cargo-dist, enabling prebuilt binaries for ARM64 Linux users via npm, the shell installer, and GitHub Releases.
- d1825f9: ### Multi-Account Support

  Add support for managing multiple Google accounts with per-account credential storage.

  **New features:**

  - `--account EMAIL` global flag available on every command
  - `GOOGLE_WORKSPACE_CLI_ACCOUNT` environment variable as fallback
  - `gws auth login --account EMAIL` — associates credentials with a specific account
  - `gws auth list` — lists all registered accounts
  - `gws auth default EMAIL` — sets the default account
  - `gws auth logout --account EMAIL` — removes a specific account
  - `login_hint` in OAuth URL for automatic account pre-selection in browser
  - Email validation via Google userinfo endpoint after OAuth flow

  **Breaking change:** Existing users must run `gws auth login` again after upgrading. The credential storage format has changed from a single `credentials.enc` to per-account files (`credentials.<b64-email>.enc`) with an `accounts.json` registry.

### Patch Changes

- a6994ad: Filter out `apps.alerts` scopes from user OAuth login flow since they require service account with domain-wide delegation
- 1ad4f34: fix: replace unwrap() calls with proper error handling in MCP server

  Replaced four `unwrap()` calls in `mcp_server.rs` that could panic the MCP
  server process with graceful error handling. Also added a warning log when
  authentication silently falls back to unauthenticated mode.

- a1be14f: fix: drain stdout pipe to prevent project listing timeout during auth setup

  Fixed `gws auth setup` timing out at step 3 (GCP project selection) for users
  with many projects. The `gcloud projects list` stdout pipe was only read after
  the child process exited, causing a deadlock when output exceeded the OS pipe
  buffer (~64 KB). Stdout is now drained in a background thread to prevent the
  pipe from filling up.

- 364542b: fix: reject DEL character (0x7F) in input validation

  The `reject_control_chars` helper rejected bytes 0x00–0x1F but allowed
  the DEL character (0x7F), which is also an ASCII control character. This
  could allow malformed input from LLM agents to bypass validation.

- 75cec1b: Fix URL template expansion so media upload endpoints substitute path parameters and avoid iterative replacement side effects.
- ed409e3: Harden URL and path construction across helper modules (gmail/watch, modelarmor, discovery)
- 263a8e5: fix: use gcloud.cmd on Windows and show platform-correct config paths

  On Windows, gcloud is installed as `gcloud.cmd` which Rust's `Command`
  cannot find without the extension. Also replaced hardcoded `~/.config/gws/`
  in error messages with the actual platform-resolved path.

## 0.3.5

### Patch Changes

- 4bca693: fix: credential masking panic and silent token write errors

  Fixed `gws auth export` masking which panicked on short strings and showed
  the entire secret instead of masking it. Also fixed silent token cache write
  failures in `save_to_disk` that returned `Ok(())` even when the write failed.

- f84ce37: Fix URL template path expansion to safely encode path parameters, including
  Sheets `range` values with Unicode and reserved characters. `{var}` expansions
  now encode as a path segment, `{+var}` preserves slashes while encoding each
  segment, and invalid path parameter/template mismatches fail fast.
- eb0347a: fix: correct author email typo in package.json
- 70d0cdd: Fix Slides presentations.get failure caused by flatPath placeholder mismatch

  When a Discovery Document's `flatPath` uses placeholder names that don't match
  the method's parameter names (e.g., `{presentationsId}` vs `presentationId`),
  `build_url` now falls back to the `path` field which uses RFC 6570 operators
  that resolve correctly.

  Fixes #118

- 37ab483: Add flake.nix for nix & NixOS installs
- 1991d53: Add prominent disclaimer that this is not an officially supported Google product to README, --help, and --version output

## 0.3.4

### Patch Changes

- 704928b: fix(setup): enable APIs individually and surface gcloud errors

  Previously `gws auth setup` used a single batch `gcloud services enable` call
  for all Workspace APIs. If any one API failed, the entire batch was marked as
  failed and stderr was silently discarded. APIs are now enabled individually and
  in parallel, with error messages surfaced to the user.

## 0.3.3

### Patch Changes

- 92e66a3: Add `gws version` as a bare subcommand alongside `gws --version` and `gws -V`

## 0.3.2

### Patch Changes

- 8fadbd6: Smarter truncation of method and resource descriptions from discovery docs. Descriptions now truncate at sentence boundaries when possible, fall back to word boundaries with an ellipsis, and strip markdown links to reclaim character budget. Fixes #64.

## 0.3.1

### Patch Changes

- b3669e0: Add hourly cron to generate-skills workflow to auto-sync skills with upstream Google Discovery API changes via PR
- e8d533e: Add workflow to publish OpenClaw skills to ClawHub
- 3b38c8d: Sync generated skills with latest Google Discovery API specs

## 0.3.0

### Minor Changes

- 670267f: feat: add `gws mcp` Model Context Protocol server

  Adds a new `gws mcp` subcommand that starts an MCP server over stdio,
  exposing Google Workspace APIs as structured tools to any MCP-compatible
  client (Claude Desktop, Gemini CLI, VS Code, etc.).

### Patch Changes

- 8c1042a: Fix x-goog-api-client header format to use `gl-rust/gws-<version>`
- 3de9762: Fix docs: `gws setup` → `gws auth setup` (fixes #56, #57)

## 0.2.2

### Patch Changes

- f281797: docs(auth): add manual Google Cloud OAuth client setup and browser-assisted login guidance

  Adds step-by-step guidance for creating a Desktop OAuth client in Google Cloud Console,
  where to place `client_secret.json`, and how humans/agents can complete browser consent
  (including unverified app and scope-selection prompts).

- ee2e216: Narrow default OAuth scopes to avoid `Error 403: restricted_client` on unverified apps and add a `--full` flag for broader access (fixes #25). Replace the cryptic non-interactive setup error with actionable step-by-step OAuth console instructions (fixes #24).
- de2787e: feat(error): detect disabled APIs and guide users to enable them

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

- 9935dde: ci: auto-generate and commit skills on PR branch pushes
- 4b868c7: docs: add community guidance to gws-shared skill and gws --help output

  Encourages agents and users to star the repository and directs bug reports
  and feature requests to GitHub Issues, with guidance to check for existing
  issues before opening new ones.

- 0603bce: fix: atomic credential file writes to prevent corruption on crash or Ctrl-C
- 666f9a8: fix(auth): support --help / -h flag on auth subcommand
- bcd2401: fix: flatten nested objects in table output and fix multi-byte char truncation panic
- ee35e4a: fix: warn to stderr when unknown --format value is provided
- e094b02: fix: YAML block scalar for strings with `#`/`:`, and repeated CSV/table headers with `--page-all`

  **Bug 1 — YAML output: `drive#file` rendered as block scalar**

  Strings containing `#` or `:` (e.g. `drive#file`, `https://…`) were
  incorrectly emitted as YAML block scalars (`|`), producing output like:

  ```yaml
  kind: |
    drive#file
  ```

  Block scalars add an implicit trailing newline which changes the string
  value and produces invalid-looking output. The fix restricts block
  scalar to strings that genuinely contain newlines; all other strings
  are double-quoted, which is safe for any character sequence.

  **Bug 2 — `--page-all` with `--format csv` / `--format table` repeats headers**

  When paginating with `--page-all`, each page printed its own header row,
  making the combined output unusable for downstream processing:

  ```
  id,kind,name          ← page 1 header
  1,drive#file,foo.txt
  id,kind,name          ← page 2 header (unexpected!)
  2,drive#file,bar.txt
  ```

  Column headers (and the table separator line) are now emitted only for
  the first page; continuation pages contain data rows only.

- 173d155: fix: add YAML document separators (---) when paginating with --page-all --format yaml
- 214fc18: ci: skip smoketest on fork pull requests

## 0.2.1

### Patch Changes

- 6ae7427: fix(auth): stabilize encrypted credential key fallback across sessions

  When the OS keyring returned `NoEntry`, the previous code could generate
  a fresh random key on each process invocation instead of reusing one.
  This caused `credentials.enc` written by `gws auth login` to be
  unreadable by subsequent commands.

  Changes:

  - Always prefer an existing `.encryption_key` file before generating a new key
  - When generating a new key, persist it to `.encryption_key` as a stable fallback
  - Best-effort write new keys into the keyring as well
  - Fix `OnceLock` race: return the already-cached key if `set` loses a race

  Fixes #27

## 0.2.0

### Minor Changes

- b0d0b95: Add workflow helpers, personas, and 50 consumer-focused recipes

  - Add `gws workflow` subcommand with 5 built-in helpers: `+standup-report`, `+meeting-prep`, `+email-to-task`, `+weekly-digest`, `+file-announce`
  - Add 10 agent personas (exec-assistant, project-manager, sales-ops, etc.) with curated skill sets
  - Add `docs/skills.md` skills index and `registry/recipes.yaml` with 50 multi-step recipes for Gmail, Drive, Docs, Calendar, and Sheets
  - Update README with skills index link and accurate skill count
  - Fix lefthook pre-commit to run fmt and clippy sequentially

### Patch Changes

- 90adcb4: fix: percent-encode path parameters to prevent path traversal
- e71ce29: Fix Gemini extension installation issue by removing redundant authentication settings and update the documentation.
- 90adcb4: fix: harden input validation for AI/LLM callers

  - Add `src/validate.rs` with `validate_safe_output_dir`, `validate_msg_format`, and `validate_safe_dir_path` helpers
  - Validate `--output-dir` against path traversal in `gmail +watch` and `events +subscribe`
  - Validate `--msg-format` against allowlist (full, metadata, minimal, raw) in `gmail +watch`
  - Validate `--dir` against path traversal in `script +push`
  - Add clap `value_parser` constraint for `--msg-format`
  - Document input validation patterns in `AGENTS.md`

- 90adcb4: Security: Harden validate_resource_name and fix Gmail watch path traversal
- 90adcb4: Replace manual `urlencoded()` with reqwest `.query()` builder for safer URL encoding
- c11d3c4: Added test coverage for `EncryptedTokenStorage::new` initialization.
- 7664357: Add test for missing error path in load_client_config
- 90adcb4: fix: add shared URL safety helpers for path params (`encode_path_segment`, `validate_resource_name`)
- 90adcb4: fix: warn on stderr when API calls fail silently

## 0.1.5

### Patch Changes

- d29f41e: Fix README typography and spacing

## 0.1.4

### Patch Changes

- adb2cfa: Fix OAuth login failing with "no refresh token" error by decrypting the token cache before parsing and supporting the HashMap token format used by EncryptedTokenStorage
- d990dcc: Improve README branding by making the hero banner full-width.

## 0.1.3

### Patch Changes

- c714f4b: Fix npm package name to publish as @googleworkspace/cli instead of gws

## 0.1.2

### Patch Changes

- 3cd4d52: Fix release pipeline to sync Cargo.toml version with changesets and create git tags for private packages

## 0.1.1

### Patch Changes

- a0ad089: Speed up CI builds with Swatinem/rust-cache, sccache, and build artifact reuse for smoketests
- 30d929b: Optimize demo GIF and improve README

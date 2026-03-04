# @googleworkspace/cli

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

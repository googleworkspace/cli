# @googleworkspace/cli

## 0.22.2

### Patch Changes

- a52d297: Improve proxy-aware OAuth flows and clean up review feedback for auth login.

## 0.22.1

### Patch Changes

- 6a45832: Sync generated skills with latest Google Discovery API specs

## 0.22.0

### Minor Changes

- 0850c48: Add `--draft` flag to Gmail `+send`, `+reply`, `+reply-all`, and `+forward` helpers to save messages as drafts instead of sending them immediately

## 0.21.2

### Patch Changes

- c4448b9: Add crates.io publishing to release workflow

  Publishes both `google-workspace` and `google-workspace-cli` to crates.io on each release. The library crate is published first (as a dependency), followed by the CLI crate.

## 0.21.1

### Patch Changes

- ea0849a: Fix version-sync script and bump CLI crate version to 0.21.0

  The `version-sync.sh` script was updating the root `Cargo.toml` which no longer has a `[package]` section after the workspace refactor. Updated to target `crates/google-workspace-cli/Cargo.toml`. Also syncs the CLI crate version to 0.21.0 to match `package.json`.

## 0.21.0

### Minor Changes

- 029e5de: Extract `google-workspace` library crate for programmatic Rust API access (closes #386)

  Introduces a Cargo workspace with a new `google-workspace` library crate (`crates/google-workspace/`)
  that exposes the core modules for use as a Rust dependency:

  - `discovery` — Discovery Document types and fetching
  - `error` — Structured `GwsError` type
  - `services` — Service registry and resolution
  - `validate` — Input validation and URL encoding
  - `client` — HTTP client with retry logic

  The `gws` binary crate re-exports all library types transparently — zero behavioral changes.

## 0.20.1

### Patch Changes

- b8fd3d9: fix(client): add 10s connect timeout to prevent hangs on initial connection
- 2bfcca9: Move version from top-level SKILL.md frontmatter to metadata and track CLI version
- 2ddb46e: test(gmail): add regression tests for RFC 2822 display name quoting
- 75a7121: Sync generated skills with latest Google Discovery API specs

## 0.20.0

### Minor Changes

- e782dd7: Forward original attachments by default and preserve inline images in HTML mode.

  `+forward` now includes the original message's attachments and inline images by default,
  matching Gmail web behavior. Use `--no-original-attachments` to opt out.
  `+reply`/`+reply-all` with `--html` preserve inline images in the quoted body via
  `multipart/related`. In plain-text mode, inline images are not included (matching Gmail web).

## 0.19.0

### Minor Changes

- a078945: Refactor all `gws auth` subcommands to use clap for argument parsing

  Replace manual argument parsing in `handle_auth_command`, `handle_login`, `resolve_scopes`, and `handle_export` with structured `clap::Command` definitions. Introduces `ScopeMode` enum for type-safe scope selection and adds proper `--help` support for all auth subcommands.

### Patch Changes

- 8a749c2: feat(helpers): add --dry-run support to events helper commands

  Add dry-run mode to `gws events +renew` and `gws events +subscribe` commands.
  When --dry-run is specified, the commands will print what actions would be
  taken without making any API calls. This allows agents to simulate requests
  and learn without reaching the server.

- d679401: Fix `mask_secret` panic on multi-byte UTF-8 secrets by using char-based indexing instead of byte-offset slicing
- d341de2: Handle --help/-h in `gws auth setup` before launching the setup wizard, preventing accidental project creation when users just want usage info
- f157208: fix: use block-style YAML sequences in generated SKILL.md frontmatter

  Replace flow sequences (`bins: ["gws"]`, `skills: [...]`) with block-style
  sequences (`bins:\n  - gws`) in all generated SKILL.md frontmatter templates.

  Flow sequences are valid YAML but rejected by `strictyaml`, which the
  Agent Skills reference implementation (`agentskills validate`) uses to parse
  frontmatter. This caused all 93 generated skills to fail validation.

  Fixes #521

- b4d5e26: Fix auth error propagation: properly propagate errors when token directory creation or permission setting fails, instead of silently ignoring them

## 0.18.1

### Patch Changes

- a87037b: Handle SIGTERM in `gws gmail +watch` and `gws events +subscribe` for clean container shutdown.

  Long-running pull loops now exit gracefully on SIGTERM (in addition to Ctrl+C),
  enabling clean shutdown under Kubernetes, Docker, and systemd.

## 0.18.0

### Minor Changes

- 908cf73: feat(gmail): auto-populate From header with display name from send-as settings

  Fetch the user's send-as identities to set the From header with a display name in all mail helpers (+send, +reply, +reply-all, +forward), matching Gmail web client behavior. Also enriches bare `--from` emails with their configured display name.

- 6e4daaf: Gmail helpers rollup: mail-builder migration, --attach flag (upload endpoint), +read helper

  - Migrate `+send`, `+reply`, `+reply-all`, and `+forward` to the `mail-builder` crate for RFC-compliant MIME construction
  - Add `--from` flag to `+send` for send-as alias support
  - Add `-a`/`--attach` flag to all mail helpers (`+send`, `+reply`, `+reply-all`, `+forward`) with `mime_guess2` auto-detection, 25MB size validation, and upload endpoint support (35MB API limit vs 5MB metadata-only)
  - Add `+read` helper to extract message body and headers (text, HTML, or JSON output)
  - Make `OriginalMessage.thread_id` optional (`Option<String>`) for draft compatibility
  - RFC 2822 display name quoting is handled natively by `mail-builder`
  - Introduce `UploadSource` enum in executor for type-safe upload strategies

### Patch Changes

- 1e90380: fix(gmail): remove dead `--attachment` arg from `+send`

  The `+send` subcommand defined a duplicate `"attachment"` arg alongside the
  `"attach"` arg already provided by `common_mail_args`. Since `parse_attachments`
  reads `"attach"`, the `--attachment` flag was silently ignored. Removed the
  dead duplicate.

- 908cf73: fix(gmail): handle reply-all to own message correctly

  Reply-all to a message you sent no longer errors with "No To recipient remains." The original To recipients are now used as reply targets, matching Gmail web client behavior.

- 2e909ae: Consolidate terminal sanitization, coloring, and output helpers into a new `output.rs` module. Fixes raw ANSI escape codes in `watch.rs` that bypassed `NO_COLOR` and TTY detection, upgrades `sanitize_for_terminal` to also strip dangerous Unicode characters (bidi overrides, zero-width spaces, directional isolates), and sanitizes previously raw API error body and user query outputs.

## 0.17.0

### Minor Changes

- 1b0a21f: feat: support google meet video conferencing in calendar +insert

### Patch Changes

- 811fe7b: Fix critical security vulnerability (TOCTOU/Symlink race) in atomic file writes.

  The atomic_write and atomic_write_async utilities now use:

  - Randomized temporary filenames to prevent predictability.
  - O_EXCL creation flags to prevent following pre-existing symlinks.
  - Strict 0600 permissions from the moment of file creation on Unix systems.
  - Redundant post-write permission calls have been removed to close race windows.

- b241a5b: fix(security): cap Retry-After sleep, sanitize upload mimeType, and validate --upload/--output paths
- 6f92e5b: Stderr/output hygiene rollup: route diagnostics to stderr, add colored error labels, propagate auth errors.

  - **triage.rs**: "No messages found" sent to stderr so stdout stays valid JSON for pipes
  - **modelarmor.rs**: response body printed only on success; error message now includes body for diagnostics
  - **error.rs**: colored `error[variant]:` labels on stderr (respects `NO_COLOR` env var), `hint:` prefix for accessNotConfigured guidance
  - **calendar, chat, docs, drive, script, sheets**: auth failures now propagate as `GwsError::Auth` instead of silently proceeding unauthenticated (dry-run still works without auth)

- 398e80c: Sync generated skills with latest Google Discovery API specs
- 8458104: Extend input validation to reject dangerous Unicode characters (zero-width chars, bidi overrides, Unicode line/paragraph separators) that were not caught by the previous ASCII-range check

## 0.16.0

### Minor Changes

- 47afe5f: Use Google account timezone instead of machine-local time for day-boundary calculations in calendar and workflow helpers. Adds `--timezone` flag to `+agenda` for explicit override. Timezone is fetched from Calendar Settings API and cached for 24 hours.

### Patch Changes

- c61b9cb: fix(gmail): RFC 2047 encode non-ASCII display names in To/From/Cc/Bcc headers

  Fixes mojibake when sending emails to recipients with non-ASCII display names (e.g. Japanese, Spanish accented characters). The new `encode_address_header()` function parses mailbox lists, encodes only the display-name portion via RFC 2047 Base64, and leaves email addresses untouched.

## 0.15.0

### Minor Changes

- 6f3e090: Add opt-in structured HTTP request logging via `tracing`

  New environment variables:

  - `GOOGLE_WORKSPACE_CLI_LOG`: stderr log filter (e.g., `gws=debug`)
  - `GOOGLE_WORKSPACE_CLI_LOG_FILE`: directory for JSON log files with daily rotation

  Logging is completely silent by default (zero overhead). Only PII-free metadata is logged: API method ID, HTTP method, status code, latency, and content-type.

## 0.14.0

### Minor Changes

- dc561e0: Add `--upload-content-type` flag and smart MIME inference for multipart uploads

  Previously, multipart uploads used the metadata `mimeType` field for both the Drive
  metadata and the media part's `Content-Type` header. This made it impossible to upload
  a file in one format (e.g. Markdown) and have Drive convert it to another (e.g. Google Docs),
  because the media `Content-Type` and the target `mimeType` must differ for import conversions.

  The new `--upload-content-type` flag allows setting the media `Content-Type` explicitly.
  When omitted, the media type is now inferred from the file extension before falling back
  to the metadata `mimeType`. This matches Google Drive's model where metadata `mimeType`
  is the _target_ type (what the file should become) while the media `Content-Type` is the
  _source_ type (what the bytes are).

  This means import conversions now work automatically:

  ```bash
  # Extension inference detects text/markdown → conversion just works
  gws drive files create \
    --json '{"name":"My Doc","mimeType":"application/vnd.google-apps.document"}' \
    --upload notes.md

  # Explicit flag still available as an override
  gws drive files create \
    --json '{"name":"My Doc","mimeType":"application/vnd.google-apps.document"}' \
    --upload notes.md \
    --upload-content-type text/markdown
  ```

### Patch Changes

- 945ac91: Stream multipart uploads to avoid OOM on large files. File content is now streamed in chunks via `ReaderStream` instead of being read entirely into memory, reducing memory usage from O(file_size) to O(64 KB).

## 0.13.3

### Patch Changes

- 8ef27a2: fix(calendar): use local timezone for agenda day boundaries instead of UTC
- 4d7b420: Fix `+append --json-values` flattening multi-row arrays into a single row by preserving the `Vec<Vec<String>>` row structure through to the API request body
- bb94016: fix(security): validate space name in chat +send to prevent path traversal
- 4b827cd: chore: fix maintainer email typo in flake.nix and harden coverage.sh
- 44767ed: Map People service to `contacts` and `directory` scope prefixes so `gws auth login -s people` includes the required OAuth scopes
- 8fce003: fix(docs): correct flag names in recipes (--spreadsheet-id, --attendees, --duration)
- 21b1840: Expose `repeated: true` in `gws schema` output and expand JSON arrays into repeated query parameters for `repeated` fields
- 1346d47: Sync generated skills with latest Google Discovery API specs
- 957b999: test(gmail): add unit tests for +triage argument parsing and format selection

## 0.13.2

### Patch Changes

- 3dcf818: Refresh OAuth access tokens for long-running Gmail watch and Workspace Events subscribe helpers before each Pub/Sub and Gmail request.
- 86ea6de: Validate `--subscription` resource name in `gmail +watch` and deduplicate `PUBSUB_API_BASE` constant.

## 0.13.1

### Patch Changes

- 510024f: Centralize token cache filenames as constants and support ServiceAccount credentials at the default plaintext path
- 510024f: Auto-recover from stale encrypted credentials after upgrade: remove undecryptable `credentials.enc` and fall through to other credential sources (plaintext, ADC) instead of hard-erroring. Also sync encryption key file backup when keyring has key but file is missing.
- e104106: Add shell tips section to gws-shared skill warning about zsh `!` history expansion, and replace single quotes with double quotes around sheet ranges containing `!` in recipes and skill examples

## 0.13.0

### Minor Changes

- 9d937af: Add `--html` flag to `+send`, `+reply`, `+reply-all`, and `+forward` for HTML email composition.

### Patch Changes

- 2df32ee: Document helper commands (`+` prefix) in README

  Adds a "Helper Commands" section to the Advanced Usage chapter explaining
  the `+` prefix convention, listing all 24 helper commands across 10 services
  with descriptions and usage examples.

## 0.12.0

### Minor Changes

- 247e27a: Add structured exit codes for scriptable error handling

  `gws` now exits with a type-specific code instead of always using `1`:

  | Code | Meaning                                                         |
  | ---- | --------------------------------------------------------------- |
  | `0`  | Success                                                         |
  | `1`  | API error — Google returned a 4xx/5xx response                  |
  | `2`  | Auth error — credentials missing, expired, or invalid           |
  | `3`  | Validation error — bad arguments, unknown service, invalid flag |
  | `4`  | Discovery error — could not fetch the API schema document       |
  | `5`  | Internal error — unexpected failure                             |

  Exit codes are documented in `gws --help` and in the README.

### Patch Changes

- 087066f: Fix `gws auth login` encrypted credential persistence by enabling native keyring backends for the `keyring` crate on supported desktop platforms instead of silently falling back to the in-memory mock store.

## 0.11.1

### Patch Changes

- adbca87: Fix `--format csv` for array-of-arrays responses (e.g. Sheets values API)

## 0.11.0

### Minor Changes

- 4d4b09f: Add `--cc` and `--bcc` flags to `+send`, `--to` and `--bcc` to `+reply` and `+reply-all`, and `--bcc` to `+forward`.

## 0.10.0

### Minor Changes

- 8d89325: Add `GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND` env var for explicit keyring backend selection (`keyring` or `file`). Fixes credential key loss in Docker/keyring-less environments by never deleting `.encryption_key` and always persisting it as a fallback.

### Patch Changes

- 06aa698: fix(auth): dynamically fetch scopes from Discovery docs when `-s` specifies services not in static scope lists
- 06aa698: fix(auth): format extract_scopes_from_doc and deduplicate dynamic scopes
- 5e7d120: Bring `+forward` behavior in line with Gmail's web UI: keep the forward in the sender's original thread, add a blank line between the forwarded message metadata and body, and remove the spurious closing delimiter.
- 2782cf1: Fix gmail +triage 403 error by using gmail.readonly scope instead of gmail.modify to avoid conflict with gmail.metadata scope that does not support the q parameter

## 0.9.1

### Patch Changes

- 5872dbe: Stop persisting encryption key to `.encryption_key` file when OS keyring is available. Existing file-based keys are migrated into the keyring and the file is removed on next CLI invocation.

## 0.9.0

### Minor Changes

- 7d15365: feat(gmail): add +reply, +reply-all, and +forward helpers

  Adds three new Gmail helper commands:

  - `+reply` -- reply to a message with automatic threading
  - `+reply-all` -- reply to all recipients with --remove/--cc support
  - `+forward` -- forward a message to new recipients

### Patch Changes

- 08716f8: Fix garbled non-ASCII email subjects in `gmail +send` by RFC 2047 encoding the Subject header and adding MIME-Version/Content-Type headers.
- f083eb9: Improve `gws auth setup` project creation failures in step 3:
  - Detect Google Cloud Terms of Service precondition failures and show actionable guidance (`gcloud auth list`, account verification, Console ToS URL).
  - Detect invalid project ID format / already-in-use errors and show clearer guidance.
  - In interactive setup, keep the wizard open and re-prompt for a new project ID instead of exiting immediately on create failures.
- 789e7f1: Switch reqwest TLS from bundled Mozilla roots to native OS certificate store

  This allows the CLI to trust custom or corporate CA certificates installed
  in the system trust store, fixing TLS errors in enterprise environments.

## 0.8.1

### Patch Changes

- 4d41e52: Prioritize local project configuration and `GOOGLE_WORKSPACE_PROJECT_ID` over global Application Default Credentials (ADC) for quota attribution. This fixes 403 errors when the Drive API is disabled in a global gcloud project but enabled in the project configured for gws.

## 0.8.0

### Minor Changes

- dd3fc90: Remove `mcp` command

## 0.7.0

### Minor Changes

- e1505af: Remove multi-account, domain-wide delegation, and impersonation support. Removes `gws auth list`, `gws auth default`, `--account` flag, `GOOGLE_WORKSPACE_CLI_ACCOUNT` and `GOOGLE_WORKSPACE_CLI_IMPERSONATED_USER` env vars.

### Patch Changes

- 54b3b31: Move x-goog-user-project header from default client headers to API request builder, fixing Discovery Document fetches failing with 403 when the quota project lacks certain APIs enabled

## 0.6.3

### Patch Changes

- 322529d: Document all environment variables and enable GOOGLE_WORKSPACE_CLI_CONFIG_DIR in release builds
- 2173a92: Send x-goog-user-project header when using ADC with a quota_project_id
- 1f47420: fix: extract CLA label job into dedicated workflow to prevent feedback loop

  The Automation workflow's `check_run: [completed]` trigger caused a feedback
  loop — every workflow completion fired a check_run event, re-triggering
  Automation, which produced another check_run event, and so on. Moving the
  CLA label job to its own `cla.yml` workflow eliminates the trigger from
  Automation entirely.

- 132c3b1: fix: warn on credential file permission failures instead of ignoring

  Replaced silent `let _ =` on `set_permissions` calls in `save_encrypted`
  with `eprintln!` warnings so users are aware if their credential files
  end up with insecure permissions. Also log keyring access failures
  instead of silently falling through to file storage.

- a2cc523: Add `x86_64-unknown-linux-musl` build target for Linux musl/static binary support
- c86b964: Fix multi-account selection: MCP server now respects `GOOGLE_WORKSPACE_CLI_ACCOUNT` env var (#221), and `--account` flag before service name no longer causes parse errors (#181)
- ff53538: Fix scope selection to use first (broadest) scope instead of all method scopes, preventing gmail.metadata restrictions from blocking query parameters
- c80eb52: Replace strip_suffix(".readonly").unwrap() with unwrap_or fallback

  Two call sites used `.strip_suffix(".readonly").unwrap()` which would
  panic if a scope URL marked as `is_readonly` didn't actually end with
  ".readonly". While the current data makes this unlikely, using
  `unwrap_or` is a defensive improvement that prevents potential panics
  from inconsistent discovery data.

- 9a780d7: Log token cache decryption/parse errors instead of silently swallowing

  Previously, `load_from_disk` used four nested `if let Ok` blocks that
  silently returned an empty map on any failure. When the encryption key
  changed or the cache was corrupted, tokens silently stopped loading and
  users were forced to re-authenticate with no explanation.

  Now logs specific warnings to stderr for decryption failures, invalid
  UTF-8, and JSON parse errors, with a hint to re-authenticate.

- 6daf90d: Fix MCP tool schemas to conditionally include `body`, `upload`, and `page_all` properties only when the underlying Discovery Document method supports them. `body` is included only when a request body is defined, `upload` only when `supportsMediaUpload` is true, and `page_all` only when the method has a `pageToken` parameter. Also drops empty `body: {}` objects that LLMs commonly send on GET methods, preventing 400 errors from Google APIs.

## 0.6.2

### Patch Changes

- 28fa25a: Clean up nits from PR #175 auth fix

  - Update stale docstring on `resolve_account` to match new fallthrough behavior
  - Add breadcrumb comment on string-based error matching in `main.rs`
  - Move identity scope injection before authenticator build for readability

## 0.6.1

### Patch Changes

- 88cb65c: chore: add automation workflow for auto-fmt, CLA labeling, and file-based PR triage
- a926e3f: Fix auth failures when accounts.json registry is missing

  Three related bugs caused all API calls to fail with "Access denied. No credentials provided" even after a successful `gws auth login`:

  1. `resolve_account()` rejected valid `credentials.enc` as "legacy" when `accounts.json` was absent, instead of using them.
  2. `main.rs` silently swallowed all auth errors, masking real failures behind a generic message.
  3. `auth login` didn't include `openid`/`email` scopes, so `fetch_userinfo_email()` couldn't identify the user, causing credentials to be saved without an `accounts.json` entry.

- cb1f988: Add Content-Length: 0 header for POST/PUT/PATCH requests with no body to fix HTTP 411 errors
- 3d59b2e: fix: isolate flaky auth tests from host ADC credentials

## 0.6.0

### Minor Changes

- b38b760: Add Application Default Credentials (ADC) support.

  `gws` now discovers ADC as a fourth credential source, after the encrypted
  and plaintext credential files. The lookup order is:

  1. `GOOGLE_WORKSPACE_CLI_TOKEN` env var (raw access token, highest priority)
  2. `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` env var
  3. Encrypted credentials (`~/.config/gws/credentials.enc`)
  4. Plaintext credentials (`~/.config/gws/credentials.json`)
  5. **ADC** — `GOOGLE_APPLICATION_CREDENTIALS` env var (hard error if file missing), then
     `~/.config/gcloud/application_default_credentials.json` (silent if absent)

  This means `gcloud auth application-default login --client-id-file=client_secret.json`
  is now a fully supported auth flow — no need to run `gws auth login` separately.
  Both `authorized_user` and `service_account` ADC formats are supported.

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

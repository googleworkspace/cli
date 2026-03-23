# Helper Commands (`+verb`) — Guidelines

## Design Principle

The core design of `gws` is **schema-driven**: commands are dynamically generated from Google Discovery Documents at runtime. This avoids maintaining a hardcoded, unbounded argument surface. **Helpers must complement this design, not duplicate it.**

## When a Helper is Justified

A `+helper` command should exist only when it provides value that Discovery-based commands **cannot**:

| Justification | Example | Why Discovery Can't Do It |
|---|---|---|
| **Multi-step orchestration** | `+subscribe` | Creates Pub/Sub topic → subscription → Workspace Events subscription (3 APIs) |
| **Format translation** | `+write` | Transforms Markdown → Docs `batchUpdate` JSON |
| **Multi-API composition** | `+triage` | Lists messages then fetches N metadata payloads concurrently |
| **Complex body construction** | `+send`, `+reply` | Builds RFC 2822 MIME from simple flags |
| **Multipart upload** | `+upload` | Handles resumable upload protocol with progress |
| **Workflow recipes** | `+standup-report` | Chains calls across multiple services |

**Litmus test:** Can the user achieve the same result with `gws <service> <resource> <method> --params '{...}'`? If yes, don't add a helper.

## Anti-Patterns

### ❌ Anti-pattern 1: Single API Call Wrapper

If a helper wraps one API call that Discovery already exposes, reject it.

**Real example:** `+revisions` (PR #563) wrapped `gws drive files-revisions list` — same single API call, zero added value.

### ❌ Anti-pattern 2: Unbounded Flag Accumulation

Adding flags to expose data that is already in the API response creates unbounded surface area.

**Real example:** `--thread-id`, `--delivered-to`, `--sent-last` on `+triage` (PR #597) — all three values are already present in the Gmail API response. Agents and users should extract them with `--format` or `jq`, not new flags.

**Why this is harmful:** Every API response contains dozens of fields. If we add a flag for each one, helpers become unbounded maintenance burdens — the exact problem Discovery-driven design solves.

### ❌ Anti-pattern 3: Duplicating Discovery Parameters

Don't re-expose Discovery-defined parameters (e.g., `pageSize`, `fields`, `orderBy`) as custom helper flags. Use `--params` passthrough instead.

## Flag Design Rules

Helper flags must control **orchestration logic**, not API parameters or output fields.

### ✅ Good Flags (control orchestration)

| Flag | Helper | Why It's Good |
|---|---|---|
| `--spreadsheet`, `--range` | `+read` | Identifies which resource to operate on |
| `--to`, `--subject`, `--body` | `+send` | Inputs to MIME construction (format translation) |
| `--dry-run` | `+subscribe` | Controls whether API calls are actually made |
| `--subscription` | `+subscribe` | Switches between "create new" vs. "use existing" orchestration path |
| `--target`, `--project` | `+subscribe` | Required for multi-service resource creation |

### ❌ Bad Flags (expose API response data)

| Flag | Why It's Bad | Alternative |
|---|---|---|
| `--thread-id` | Already in API response | `jq '.threadId'` |
| `--delivered-to` | Already in response headers | `jq '.payload.headers[] | ...'` |
| `--include-labels` | Output field filtering | `--format` or `jq` |

### Decision Checklist for New Flags

1. Does this flag control **what API call to make** or **how to orchestrate** multiple calls? → ✅ Add it
2. Does this flag control **what data appears in output**? → ❌ Use `--format`/`jq`
3. Does this flag duplicate a Discovery parameter? → ❌ Use `--params`
4. Could the user achieve this with existing flags + post-processing? → ❌ Don't add it

## Architecture

Helpers are implemented using the `Helper` trait defined in `mod.rs`.

- **`inject_commands`**: Adds subcommands to the main service command. All helper commands are always shown regardless of authentication state.
- **`handle`**: Implementation of the command logic. Returns `Ok(true)` if the command was handled, or `Ok(false)` to let the default raw resource handler attempt to handle it.

## Adding a New Helper — Checklist

1. **Passes the litmus test** — cannot be done with a single Discovery command
2. **Flags are bounded** — only flags controlling orchestration, not API params/output
3. **Uses shared infrastructure:**
   - `crate::client::build_client()` for HTTP
   - `crate::validate::validate_resource_name()` for user-supplied resource IDs
   - `crate::validate::encode_path_segment()` for URL path segments
   - `crate::output::sanitize_for_terminal()` for error messages
4. **Has tests** — at minimum: command registration, required args, happy path
5. **Supports `--dry-run`** where the helper creates or mutates resources

### Development Steps

1. Create `src/helpers/<service>.rs`
2. Implement the `Helper` trait
3. Register it in `src/helpers/mod.rs`
4. **Prefix** the command with `+` (e.g., `+create`)

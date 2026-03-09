---
name: gws-events-subscribe
version: 1.0.0
description: "Google Workspace Events: Subscribe to Workspace events and stream them as NDJSON. Supports calendar event changes, Gmail/email activity, Chat message notifications, and Drive file updates via Pub/Sub. Use when the user wants to monitor Google Workspace activity in real time, track calendar changes, receive Gmail or Chat alerts, set up Workspace webhooks, watch Drive file changes, or stream live Google Workspace event updates to a file or terminal."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws events +subscribe --help"
---

# events +subscribe

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Subscribe to Workspace events and stream them as NDJSON

## Usage

```bash
gws events +subscribe
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--target` | — | — | Workspace resource URI (e.g., //chat.googleapis.com/spaces/SPACE_ID) |
| `--event-types` | — | — | Comma-separated CloudEvents types to subscribe to |
| `--project` | — | — | GCP project ID for Pub/Sub resources |
| `--subscription` | — | — | Existing Pub/Sub subscription name (skip setup) |
| `--max-messages` | — | 10 | Max messages per pull batch (default: 10) |
| `--poll-interval` | — | 5 | Seconds between pulls (default: 5) |
| `--once` | — | — | Pull once and exit |
| `--cleanup` | — | — | Delete created Pub/Sub resources on exit |
| `--no-ack` | — | — | Don't auto-acknowledge messages |
| `--output-dir` | — | — | Write each event to a separate JSON file in this directory |

## Examples

```bash
gws events +subscribe --target '//chat.googleapis.com/spaces/SPACE' --event-types 'google.workspace.chat.message.v1.created' --project my-project
gws events +subscribe --subscription projects/p/subscriptions/my-sub --once
gws events +subscribe ... --cleanup --output-dir ./events
```

## Workflow

1. **Pre-flight: verify target URI** — Confirm the `--target` resource URI follows the expected format (e.g., `//chat.googleapis.com/spaces/SPACE_ID`) before executing.
2. **Pre-flight: verify permissions** — Run with `--once` first to confirm the authenticated account has the required Pub/Sub and Workspace permissions (see `gws-shared`) before committing to resource creation or a long-running stream.
3. Run the subscribe command with the desired `--target` and `--event-types`.
4. Confirm NDJSON events begin streaming to stdout (or files in `--output-dir`).
5. If no messages appear, verify the target resource URI is correct and the Pub/Sub subscription exists in the specified `--project`.
6. If the command fails on setup, check that the authenticated account has the required Pub/Sub and Workspace permissions (see `gws-shared`).
7. Press Ctrl-C to stop gracefully; use `--cleanup` to remove created Pub/Sub resources automatically on exit.

## Tips

- Without `--cleanup`, Pub/Sub resources persist for reconnection.
- Use `--once` to do a single pull and verify events are flowing before committing to a long-running stream.
- Press Ctrl-C to stop gracefully.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-events](../gws-events/SKILL.md) — All subscribe to google workspace events commands

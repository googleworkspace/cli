---
name: gws-gmail-watch
version: 1.0.0
description: "Monitors a Gmail inbox for new emails and streams each message as NDJSON via Google Cloud Pub/Sub. Use when the user wants to watch for incoming mail, monitor new messages or email notifications in real time, receive inbox updates as a data stream, or capture email data for automation and processing pipelines."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws gmail +watch --help"
---

# gmail +watch

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Watch for new emails and stream them as NDJSON

## Usage

```bash
gws gmail +watch
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--project` | — | — | GCP project ID for Pub/Sub resources |
| `--subscription` | — | — | Existing Pub/Sub subscription name (skip setup) |
| `--topic` | — | — | Existing Pub/Sub topic with Gmail push permission already granted |
| `--label-ids` | — | — | Comma-separated Gmail label IDs to filter (e.g., INBOX,UNREAD) |
| `--max-messages` | — | 10 | Max messages per pull batch |
| `--poll-interval` | — | 5 | Seconds between pulls |
| `--msg-format` | — | full | Gmail message format: full, metadata, minimal, raw |
| `--once` | — | — | Pull once and exit |
| `--cleanup` | — | — | Delete created Pub/Sub resources on exit |
| `--output-dir` | — | — | Write each message to a separate JSON file in this directory |

## Pre-flight Verification

Before running `+watch`, confirm Pub/Sub IAM permissions are in place — this is the most common failure point:

```bash
# Verify the Gmail push service account has Publisher rights on the topic
gcloud pubsub topics get-iam-policy TOPIC_NAME

# The output must include an entry like:
# - members: serviceAccount:gmail-api-push@system.gserviceaccount.com
#   role: roles/pubsub.publisher

# If missing, grant it:
gcloud pubsub topics add-iam-policy-binding TOPIC_NAME \
  --member="serviceAccount:gmail-api-push@system.gserviceaccount.com" \
  --role="roles/pubsub.publisher"
```

## Examples

```bash
gws gmail +watch --project my-gcp-project
gws gmail +watch --project my-project --label-ids INBOX --once
gws gmail +watch --subscription projects/p/subscriptions/my-sub
gws gmail +watch --project my-project --cleanup --output-dir ./emails
```

## Expected Output

A successfully established watch streams one NDJSON object per email to stdout:

```json
{"id":"18e1a...","threadId":"18e1a...","labelIds":["INBOX"],"snippet":"Hello...","payload":{...}}
```

If `--output-dir` is set, each message is also written to a separate `.json` file in that directory.

## Error Handling

| Error | Likely Cause | Resolution |
|-------|-------------|------------|
| `403 Forbidden` on Pub/Sub | Missing IAM permissions | Grant the `gmail-api-push@system.gserviceaccount.com` service account `Pub/Sub Publisher` on the topic |
| `project not found` | Invalid `--project` value | Verify the GCP project ID with `gcloud projects list` |
| `subscription not found` | Stale `--subscription` reference | Omit the flag to let the command create a fresh subscription, or verify the subscription exists |
| Watch silently receives no messages | Label filter too narrow | Check `--label-ids` values match actual Gmail label IDs |

## Tips

- Gmail watch expires after 7 days — re-run to renew.
- Without --cleanup, Pub/Sub resources persist for reconnection.
- Press Ctrl-C to stop gracefully.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-gmail](../gws-gmail/SKILL.md) — All send, read, and manage email commands

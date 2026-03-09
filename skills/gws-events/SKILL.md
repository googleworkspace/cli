---
name: gws-events
version: 1.0.0
description: "Manages Google Workspace Events subscriptions and notifications via the `gws events` CLI. Handles creating, listing, updating, deleting, and reactivating webhook/push-notification subscriptions that monitor changes to Gmail messages, Calendar events, Drive file changes, and other Workspace resources. Also supports streaming task updates and managing long-running operations. Use when the user wants to watch for changes, set up webhooks or push notifications, monitor Gmail/Calendar/Drive activity, subscribe to Workspace events, manage notification channels, or stream real-time event callbacks from Google Workspace products."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws events --help"
---

# events (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws events <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+subscribe`](../gws-events-subscribe/SKILL.md) | Subscribe to Workspace events and stream them as NDJSON |
| [`+renew`](../gws-events-renew/SKILL.md) | Renew/reactivate Workspace Events subscriptions |

## API Resources

### message

  - `stream` — SendStreamingMessage is a streaming call that will return a stream of task update events until the Task is in an interrupted or terminal state.

### operations

  - `get` — Gets the latest state of a long-running operation. Clients can use this method to poll the operation result at intervals as recommended by the API service.

### subscriptions

  - `create` — Creates a Google Workspace subscription. To learn how to use this method, see [Create a Google Workspace subscription](https://developers.google.com/workspace/events/guides/create-subscription).
  - `delete` — Deletes a Google Workspace subscription. To learn how to use this method, see [Delete a Google Workspace subscription](https://developers.google.com/workspace/events/guides/delete-subscription).
  - `get` — Gets details about a Google Workspace subscription. To learn how to use this method, see [Get details about a Google Workspace subscription](https://developers.google.com/workspace/events/guides/get-subscription).
  - `list` — Lists Google Workspace subscriptions. To learn how to use this method, see [List Google Workspace subscriptions](https://developers.google.com/workspace/events/guides/list-subscriptions).
  - `patch` — Updates or renews a Google Workspace subscription. To learn how to use this method, see [Update or renew a Google Workspace subscription](https://developers.google.com/workspace/events/guides/update-subscription).
  - `reactivate` — Reactivates a suspended Google Workspace subscription. This method resets your subscription's `State` field to `ACTIVE`. Before you use this method, you must fix the error that suspended the subscription. This method will ignore or reject any subscription that isn't currently in a suspended state. To learn how to use this method, see [Reactivate a Google Workspace subscription](https://developers.google.com/workspace/events/guides/reactivate-subscription).

### tasks

  - `cancel` — Cancel a task from the agent. If supported one should expect no more task updates for the task.
  - `get` — Get the current state of a task from the agent.
  - `subscribe` — TaskSubscription is a streaming call that will return a stream of task update events. This attaches the stream to an existing in process task. If the task is complete the stream will return the completed task (like GetTask) and close the stream.
  - `pushNotificationConfigs` — Operations on the 'pushNotificationConfigs' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws events --help

# Inspect a method's required params, types, and defaults
gws schema events.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Quick-Start Workflow

Typical sequence for setting up and verifying a Workspace event subscription:

**1. Discover** — inspect the method schema before building params:
```bash
gws schema events.subscriptions.create
```

**2. Create** — create a subscription (e.g. watch for Gmail new-message events):
```bash
gws events subscriptions create \
  --json '{
    "targetResource": "//mail.googleapis.com/users/me/messages",
    "eventTypes": ["google.workspace.gmail.message.v1.created"],
    "notificationEndpoint": {
      "pubsubTopic": "projects/my-project/topics/my-topic"
    },
    "ttl": "86400s"
  }'
```

> **If creation fails:** Check that the `pubsubTopic` exists and that the Workspace Events service account has Pub/Sub Publisher permission on it. Permission errors typically surface as `PERMISSION_DENIED`; missing topics appear as `NOT_FOUND`.

**3. Verify** — confirm the subscription is active:
```bash
gws events subscriptions get --params '{"name": "subscriptions/SUBSCRIPTION_ID"}'
```

> **If the subscription is `SUSPENDED`:** Inspect the `suspensionReason` field in the response, fix the underlying issue (e.g. expired credentials, revoked Pub/Sub access), then reactivate with `gws events subscriptions reactivate`.

**4. Stream events** — attach to a live task stream for real-time updates:
```bash
gws events tasks subscribe --params '{"name": "tasks/TASK_ID"}'
```

Use the [`+subscribe`](../gws-events-subscribe/SKILL.md) helper for a higher-level streaming workflow that outputs NDJSON.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---------|--------------|------------|
| `PERMISSION_DENIED` on create | Service account lacks Pub/Sub Publisher role on the target topic | Grant the role in GCP IAM, then retry |
| Subscription state is `SUSPENDED` | Credentials expired or endpoint unreachable | Fix the root cause, then call `subscriptions reactivate` |
| `NOT_FOUND` on get/delete | Wrong subscription ID or it was already deleted | Run `subscriptions list` to confirm existing IDs |
| Stream closes immediately | Task already in terminal state | Use `tasks get` to retrieve the final task state instead |

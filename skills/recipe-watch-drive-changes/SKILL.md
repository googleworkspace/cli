---
name: recipe-watch-drive-changes
version: 1.0.0
description: "Subscribe to, manage, and renew change notifications on a Google Drive file or folder. Use when a user wants to watch for changes, monitor a Drive folder, track file updates, get alerts when files change, or set up Drive webhooks/event subscriptions. Covers creating subscriptions, listing active notifications, validating subscription state, and renewing before expiry."
metadata:
  openclaw:
    category: "recipe"
    domain: "engineering"
    requires:
      bins: ["gws"]
      skills: ["gws-events"]
---

# Watch for Drive Changes

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-events`

Subscribe to change notifications on a Google Drive file or folder.

## Steps

1. Create subscription: `gws events subscriptions create --json '{"targetResource": "//drive.googleapis.com/drives/DRIVE_ID", "eventTypes": ["google.workspace.drive.file.v1.updated"], "notificationEndpoint": {"pubsubTopic": "projects/PROJECT/topics/TOPIC"}, "payloadOptions": {"includeResource": true}}'`
2. Validate creation: confirm the response contains a `name` field (e.g. `subscriptions/SUBSCRIPTION_ID`) and `state: ACTIVE`. If the command returns an error, check for common causes:
   - **Permission denied** — ensure the service account has `pubsub.topics.publish` on the topic and Drive API access.
   - **Invalid topic** — verify `projects/PROJECT/topics/TOPIC` exists and is spelled correctly.
   - **Resource not found** — confirm `DRIVE_ID` is a valid shared drive or file ID.
3. List active subscriptions: `gws events subscriptions list`
4. Verify the new subscription appears in the list output before proceeding.
5. Renew before expiry: `gws events +renew --subscription SUBSCRIPTION_ID`

---
name: gws-admin-reports
version: 1.0.0
description: "Google Workspace Admin SDK reports_v1 skill. Retrieves audit logs, generates usage reports, tracks user activity, monitors login events, and exports admin data for a Google Workspace (G Suite) account. Covers activities, channels, customer usage, entity usage, and per-user usage. Use when the user asks about Google Workspace or G Suite admin tasks such as user activity logs, login history, security events, admin console reports, Google Drive activity, push notifications for account changes, or any G Suite administration reporting need."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws admin-reports --help"
---

# admin-reports (reports_v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws admin-reports <resource> <method> [flags]
```

## API Resources

### activities

- `list` — Retrieves a list of activities for a specific customer's account and application (e.g., Admin console, Google Drive). Use for audit logs, login history, and security events.
- `watch` — Start receiving push notifications for account activities.

### channels

- `stop` — Stop watching resources through this channel.

### customerUsageReports

- `get` — Retrieves a report of properties and statistics for a specific customer's account (customer-level usage metrics).

### entityUsageReports

- `get` — Retrieves a report of properties and statistics for entities used by users within the account (entity-level usage metrics).

### userUsageReport

- `get` — Retrieves a report of properties and statistics for a set of users within the account (per-user usage metrics).

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws admin-reports --help

# Inspect a method's required params, types, and defaults
gws schema admin-reports.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Examples

### Retrieve Admin Console activity logs for a user

```bash
# Inspect the method first
gws schema admin-reports.activities.list

# List Admin console activities for a specific user
gws admin-reports activities list \
  --params '{"userKey": "user@example.com", "applicationName": "admin", "startTime": "2024-01-01T00:00:00Z", "endTime": "2024-01-31T23:59:59Z"}'
```

### Retrieve login history (login audit log)

```bash
gws admin-reports activities list \
  --params '{"userKey": "all", "applicationName": "login", "maxResults": 100}'
```

### Get customer-level usage report

```bash
gws admin-reports customerUsageReports get \
  --params '{"date": "2024-01-15"}'
```

### Set up push notifications for account activities (watch workflow)

```bash
# 1. Inspect the watch method
gws schema admin-reports.activities.watch

# 2. Start watching — supply a channel ID, token, and your webhook address
gws admin-reports activities watch \
  --params '{"userKey": "all", "applicationName": "admin"}' \
  --json '{"id": "my-channel-01", "type": "web_hook", "address": "https://your-endpoint.example.com/notifications", "token": "my-secret-token"}'

# 3. Validate: confirm your endpoint receives the sync message Google sends on registration

# 4. Stop watching when done (use the channel id and resourceId returned in step 2)
gws admin-reports channels stop \
  --json '{"id": "my-channel-01", "resourceId": "<resourceId-from-watch-response>"}'
```

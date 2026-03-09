---
name: gws-events-renew
version: 1.0.0
description: "Google Workspace Events: Renew or reactivate Workspace Events subscriptions. Use when a subscription has expired, is expiring soon, or needs reactivation — e.g., 'renew subscription', 'reactivate events subscription', 'subscription expired', 'subscription expiring soon', 'Google Workspace subscription renewal', 'events API subscription', or batch-renewing all subscriptions within a time window."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws events +renew --help"
---

# events +renew

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Renew/reactivate Workspace Events subscriptions

## Usage

```bash
gws events +renew
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--name` | — | — | Subscription name to reactivate (e.g., subscriptions/SUB_ID) |
| `--all` | — | — | Renew all subscriptions expiring within --within window |
| `--within` | — | 1h | Time window for --all (e.g., 1h, 30m, 2d) |

## Examples

```bash
gws events +renew --name subscriptions/SUB_ID
gws events +renew --all --within 2d
```

## Output

- A successful single renewal returns the updated subscription details, including the new expiration time. Verify the renewed `expireTime` field to confirm the operation succeeded.
- With `--all`, a summary is printed listing each subscription processed along with its renewal status (success or failure). Subscriptions that fail to renew are reported individually — the batch continues for the remaining subscriptions rather than halting on the first error.

## Handling Failed Renewals (`--all`)

If one or more renewals fail in a batch run:

1. Note the failed subscription IDs from the output summary.
2. Retry each failed subscription individually using `--name`:
   ```bash
   gws events +renew --name subscriptions/FAILED_SUB_ID
   ```
3. Investigate persistent failures — the individual error output may reveal permission issues, invalid subscription state, or API errors.

## Tips

- Subscriptions expire if not renewed periodically.
- Use `--all` with a cron job to keep subscriptions alive.
- When using `--all`, review the output summary to catch any individual renewal failures before assuming all subscriptions are active.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-events](../gws-events/SKILL.md) — All subscribe to google workspace events commands

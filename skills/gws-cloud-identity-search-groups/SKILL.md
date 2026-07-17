---
name: gws-cloud-identity-search-groups
description: "Cloud Identity: Search which Google Groups a user belongs to (no admin privileges required)."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
    cliHelp: "gws cloud-identity groups memberships searchDirectGroups --help"
---

# cloud-identity groups memberships searchDirectGroups

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Lists the Google Groups (and other Cloud Identity groups) a given user directly
belongs to — the same view as the "My Groups" page at
https://groups.google.com. Unlike the Admin SDK Directory API, this works for
any authenticated user asking about their own membership: no super-admin or
delegated-admin role is required. Groups the caller isn't allowed to see are
silently omitted from the response rather than causing an error.

## Usage

```bash
gws cloud-identity groups memberships searchDirectGroups \
  --params '{"parent":"groups/-","query":"member_key_id == '\''<email>'\''"}'
```

`parent` is always the literal string `groups/-` (the API searches across all
groups for the given member — it is not a real resource ID). `query` is a CEL
expression; at minimum set `member_key_id` to the email address you want to
search.

## Required scope

Needs the `https://www.googleapis.com/auth/cloud-identity.groups.readonly`
OAuth scope (or the read/write `cloud-identity.groups` scope) and the Cloud
Identity API enabled on the backing GCP project. Neither Admin SDK Directory
scopes nor a Workspace admin role are required — that's the point of this
endpoint over the Admin Directory API's `groups.list`.

## Known gotcha: the documented label filter breaks

Google's own docs show a query like:

```
member_key_id == '<email>' && 'cloudidentity.googleapis.com/groups.discussion_forum' in labels
```

As of this API version, adding that `&& '...' in labels` clause returns a
`400 INVALID_ARGUMENT` (confirmed against the live API directly, bypassing
`gws`, to rule out a CLI bug). Use the bare `member_key_id == '<email>'`
clause instead — it already returns only the groups the caller can see, which
in practice are the Google Groups the user belongs to.

## Examples

```bash
# Your own group memberships
gws cloud-identity groups memberships searchDirectGroups \
  --params '{"parent":"groups/-","query":"member_key_id == '\''me@example.com'\''"}'

# Trim the response and paginate if the user belongs to 100+ groups
gws cloud-identity groups memberships searchDirectGroups \
  --params '{"parent":"groups/-","query":"member_key_id == '\''me@example.com'\''","fields":"memberships(groupKey,displayName,roles),nextPageToken"}' \
  --page-all
```

## Tips

- Read-only — never modifies group membership.
- `searchTransitiveGroups` (nested/group-of-a-group membership) lives on the
  same resource but requires a Workspace Enterprise or Cloud Identity Premium
  SKU — it returned `403 PERMISSION_DENIED` on a standard account in testing.
  Prefer `searchDirectGroups` unless you specifically need transitive
  membership and know your SKU supports it.
- For anything beyond the caller's own membership (auditing another user's
  groups, listing a group's full roster), the caller needs visibility into
  that group's membership — Google filters out what it can't see rather than
  erroring.
- `--fields` is a query parameter, not a CLI flag — put it inside `--params`,
  not on the command line.

## See Also

- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth
- [gws-cloud-identity](../gws-cloud-identity/SKILL.md) — All Cloud Identity groups/devices/policies commands

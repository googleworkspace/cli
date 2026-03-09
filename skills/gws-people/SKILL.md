---
name: gws-people
version: 1.0.0
description: "Google People API skill for managing Google Contacts, address books, and user profiles via the `gws` CLI. Use when a user wants to create, search, update, or delete contacts or contact groups; look up phone numbers or email addresses in their Google address book; manage their contact list or directory; sync contacts; retrieve profile photos; or copy contacts between groups. Supports contact groups (create, list, update, delete, manage members), other contacts (list, search, copy to My Contacts), and people/connections (batch create/update, search contacts, list directory people, update photos)."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws people --help"
---

# people (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws people <resource> <method> [flags]
```

## API Resources

### contactGroups

  - `batchGet` — Get a list of contact groups owned by the authenticated user by specifying a list of contact group resource names.
  - `create` — Create a new contact group owned by the authenticated user. Created contact group names must be unique to the users contact groups. Attempting to create a group with a duplicate name will return a HTTP 409 error. Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `delete` — Delete an existing contact group owned by the authenticated user by specifying a contact group resource name. Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `get` — Get a specific contact group owned by the authenticated user by specifying a contact group resource name.
  - `list` — List all contact groups owned by the authenticated user. Members of the contact groups are not populated.
  - `update` — Update the name of an existing contact group owned by the authenticated user. Updated contact group names must be unique to the users contact groups. Attempting to create a group with a duplicate name will return a HTTP 409 error. Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `members` — Operations on the 'members' resource

### otherContacts

  - `copyOtherContactToMyContactsGroup` — Copies an "Other contact" to a new contact in the user's "myContacts" group Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `list` — List all "Other contacts", that is contacts that are not in a contact group. "Other contacts" are typically auto created contacts from interactions. Sync tokens expire 7 days after the full sync. A request with an expired sync token will get an error with an [google.rpc.ErrorInfo](https://cloud.google.com/apis/design/errors#error_info) with reason "EXPIRED_SYNC_TOKEN". In the case of such an error clients should make a full sync request without a `sync_token`.
  - `search` — Provides a list of contacts in the authenticated user's other contacts that matches the search query. The query matches on a contact's `names`, `emailAddresses`, and `phoneNumbers` fields that are from the OTHER_CONTACT source. **IMPORTANT**: Before searching, clients should send a warmup request with an empty query to update the cache. See https://developers.google.com/people/v1/other-contacts#search_the_users_other_contacts

### people

  - `batchCreateContacts` — Create a batch of new contacts and return the PersonResponses for the newly Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `batchUpdateContacts` — Update a batch of contacts and return a map of resource names to PersonResponses for the updated contacts. Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `createContact` — Create a new contact and return the person resource for that contact. The request returns a 400 error if more than one field is specified on a field that is a singleton for contact sources: * biographies * birthdays * genders * names Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `deleteContactPhoto` — Delete a contact's photo. Mutate requests for the same user should be done sequentially to avoid // lock contention.
  - `get` — Provides information about a person by specifying a resource name. Use `people/me` to indicate the authenticated user. The request returns a 400 error if 'personFields' is not specified.
  - `getBatchGet` — Provides information about a list of specific people by specifying a list of requested resource names. Use `people/me` to indicate the authenticated user. The request returns a 400 error if 'personFields' is not specified.
  - `listDirectoryPeople` — Provides a list of domain profiles and domain contacts in the authenticated user's domain directory. When the `sync_token` is specified, resources deleted since the last sync will be returned as a person with `PersonMetadata.deleted` set to true. When the `page_token` or `sync_token` is specified, all other request parameters must match the first call. Writes may have a propagation delay of several minutes for sync requests. Incremental syncs are not intended for read-after-write use cases.
  - `searchContacts` — Provides a list of contacts in the authenticated user's grouped contacts that matches the search query. The query matches on a contact's `names`, `nickNames`, `emailAddresses`, `phoneNumbers`, and `organizations` fields that are from the CONTACT source. **IMPORTANT**: Before searching, clients should send a warmup request with an empty query to update the cache. See https://developers.google.com/people/v1/contacts#search_the_users_contacts
  - `searchDirectoryPeople` — Provides a list of domain profiles and domain contacts in the authenticated user's domain directory that match the search query.
  - `updateContact` — Update contact data for an existing contact person. Any non-contact data will not be modified. Any non-contact data in the person to update will be ignored. All fields specified in the `update_mask` will be replaced. The server returns a 400 error if `person.metadata.sources` is not specified for the contact to be updated or if there is no contact source.
  - `updateContactPhoto` — Update a contact's photo. Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `connections` — Operations on the 'connections' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws people --help

# Inspect a method's required params, types, and defaults
gws schema people.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

### Example: inspect then construct a command

```bash
# 1. Check what fields createContact requires
gws schema people.people.createContact

# 2. Build the command from the schema output
gws people people createContact \
  --json '{"names":[{"givenName":"Jane","familyName":"Doe"}],"emailAddresses":[{"value":"jane.doe@example.com"}]}'
```

## Common Usage Examples

### List all contact groups

```bash
gws people contactGroups list
```

### Create a new contact group

```bash
gws people contactGroups create \
  --json '{"contactGroup":{"name":"Team Alpha"}}'
```

### Create a single contact

```bash
gws people people createContact \
  --json '{"names":[{"givenName":"Jane","familyName":"Doe"}],"emailAddresses":[{"value":"jane.doe@example.com"}],"phoneNumbers":[{"value":"+1-555-0100","type":"mobile"}]}'
```

### Search contacts by name or email

```bash
# Warmup request first (required before searching)
gws people people searchContacts --params 'query=&readMask=names,emailAddresses'

# Then perform the actual search
gws people people searchContacts --params 'query=Jane&readMask=names,emailAddresses,phoneNumbers'
```

### Get authenticated user's own profile

```bash
gws people people get \
  --params 'resourceName=people/me&personFields=names,emailAddresses,phoneNumbers'
```

## Mutate Operation Workflow

Many write operations (create, update, delete, batch operations) carry this warning: **mutate requests for the same user must be sent sequentially** to avoid increased latency and failures.

Follow this pattern for mutate operations:

1. **Inspect the schema** before building the request body.
2. **Send one mutate request at a time** per user — do not parallelize creates/updates/deletes.
3. **Handle common errors**:
   - `400` — Missing required fields (e.g., `personFields` not specified, or singleton fields duplicated). Re-inspect with `gws schema` and correct the request body.
   - `409` — Duplicate contact group name. Choose a unique name before retrying.
   - `EXPIRED_SYNC_TOKEN` — Sync token is older than 7 days. Discard the token and perform a full sync (omit `sync_token` from the request).
4. **Verify the result** by running a `get` or `list` command after a successful mutate.

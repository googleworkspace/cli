---
name: gws-people
version: 1.0.0
description: "Google People: Manage contacts and profiles."
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
  - `create` — Create a new contact group owned by the authenticated user. Created contact group names must be unique to the users contact groups. Attempting to create a group with a duplicate name will return a HTT
  - `delete` — Delete an existing contact group owned by the authenticated user by specifying a contact group resource name. Mutate requests for the same user should be sent sequentially to avoid increased latency a
  - `get` — Get a specific contact group owned by the authenticated user by specifying a contact group resource name.
  - `list` — List all contact groups owned by the authenticated user. Members of the contact groups are not populated.
  - `update` — Update the name of an existing contact group owned by the authenticated user. Updated contact group names must be unique to the users contact groups. Attempting to create a group with a duplicate name
  - `members` — Operations on the 'members' resource

### otherContacts

  - `copyOtherContactToMyContactsGroup` — Copies an "Other contact" to a new contact in the user's "myContacts" group Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `list` — List all "Other contacts", that is contacts that are not in a contact group. "Other contacts" are typically auto created contacts from interactions. Sync tokens expire 7 days after the full sync. A re
  - `search` — Provides a list of contacts in the authenticated user's other contacts that matches the search query. The query matches on a contact's `names`, `emailAddresses`, and `phoneNumbers` fields that are fro

### people

  - `batchCreateContacts` — Create a batch of new contacts and return the PersonResponses for the newly Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `batchDeleteContacts` — Delete a batch of contacts. Any non-contact data will not be deleted. Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `batchUpdateContacts` — Update a batch of contacts and return a map of resource names to PersonResponses for the updated contacts. Mutate requests for the same user should be sent sequentially to avoid increased latency and 
  - `createContact` — Create a new contact and return the person resource for that contact. The request returns a 400 error if more than one field is specified on a field that is a singleton for contact sources: * biograph
  - `deleteContact` — Delete a contact person. Any non-contact data will not be deleted. Mutate requests for the same user should be sent sequentially to avoid increased latency and failures.
  - `deleteContactPhoto` — Delete a contact's photo. Mutate requests for the same user should be done sequentially to avoid // lock contention.
  - `get` — Provides information about a person by specifying a resource name. Use `people/me` to indicate the authenticated user. The request returns a 400 error if 'personFields' is not specified.
  - `getBatchGet` — Provides information about a list of specific people by specifying a list of requested resource names. Use `people/me` to indicate the authenticated user. The request returns a 400 error if 'personFie
  - `listDirectoryPeople` — Provides a list of domain profiles and domain contacts in the authenticated user's domain directory. When the `sync_token` is specified, resources deleted since the last sync will be returned as a per
  - `searchContacts` — Provides a list of contacts in the authenticated user's grouped contacts that matches the search query. The query matches on a contact's `names`, `nickNames`, `emailAddresses`, `phoneNumbers`, and `or
  - `searchDirectoryPeople` — Provides a list of domain profiles and domain contacts in the authenticated user's domain directory that match the search query.
  - `updateContact` — Update contact data for an existing contact person. Any non-contact data will not be modified. Any non-contact data in the person to update will be ignored. All fields specified in the `update_mask` w
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


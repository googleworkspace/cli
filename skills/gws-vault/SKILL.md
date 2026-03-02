---
name: gws-vault
version: 1.0.0
description: "USE WHEN the user wants to manage ediscovery holds and exports via the `gws` CLI."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws vault --help"
---

# vault (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws vault <resource> <method> [flags]
```

## API Resources

### matters

  - `addPermissions` — Adds an account as a matter collaborator.
  - `close` — Closes the specified matter. Returns the matter with updated state.
  - `count` — Counts the accounts processed by the specified query.
  - `create` — Creates a matter with the given name and description. The initial state is open, and the owner is the method caller. Returns the created matter with default view.
  - `delete` — Deletes the specified matter. Returns the matter with updated state.
  - `get` — Gets the specified matter.
  - `list` — Lists matters the requestor has access to.
  - `removePermissions` — Removes an account as a matter collaborator.
  - `reopen` — Reopens the specified matter. Returns the matter with updated state.
  - `undelete` — Undeletes the specified matter. Returns the matter with updated state.
  - `update` — Updates the specified matter. This updates only the name and description of the matter, identified by matter ID. Changes to any other fields are ignored. Returns the default view of the matter.
  - `exports` — Operations on the 'exports' resource
  - `holds` — Operations on the 'holds' resource
  - `savedQueries` — Operations on the 'savedQueries' resource

### operations

  - `cancel` — Starts asynchronous cancellation on a long-running operation. The server makes a best effort to cancel the operation, but success is not guaranteed. If the server doesn't support this method, it retur
  - `delete` — Deletes a long-running operation. This method indicates that the client is no longer interested in the operation result. It does not cancel the operation. If the server doesn't support this method, it
  - `get` — Gets the latest state of a long-running operation. Clients can use this method to poll the operation result at intervals as recommended by the API service.
  - `list` — Lists operations that match the specified filter in the request. If the server doesn't support this method, it returns `UNIMPLEMENTED`.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws vault --help

# Inspect a method's required params, types, and defaults
gws schema vault.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


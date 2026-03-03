---
name: gws-admin
version: 1.0.0
description: "Google Workspace Admin SDK: Manage users, groups, and devices."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws admin --help"
---

# admin (directory_v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws admin <resource> <method> [flags]
```

## API Resources

### asps

  - `delete` — Deletes an ASP issued by a user.
  - `get` — Gets information about an ASP issued by a user.
  - `list` — Lists the ASPs issued by a user.

### channels

  - `stop` — Stops watching resources through this channel.

### chromeosdevices

  - `action` — Use [BatchChangeChromeOsDeviceStatus](https://developers.google.com/workspace/admin/directory/reference/rest/v1/customer.devices.chromeos/batchChangeStatus) instead. Takes an action that affects a Chr
  - `get` — Retrieves a Chrome OS device's properties.
  - `list` — Retrieves a paginated list of Chrome OS devices within an account.
  - `moveDevicesToOu` — Moves or inserts multiple Chrome OS devices to an organizational unit. You can move up to 50 devices at once.
  - `patch` — Updates a device's updatable properties, such as `annotatedUser`, `annotatedLocation`, `notes`, `orgUnitPath`, or `annotatedAssetId`. This method supports [patch semantics](https://developers.google.c
  - `update` — Updates a device's updatable properties, such as `annotatedUser`, `annotatedLocation`, `notes`, `orgUnitPath`, or `annotatedAssetId`.

### customer

  - `devices` — Operations on the 'devices' resource

### customers

  - `get` — Retrieves a customer.
  - `patch` — Patches a customer.
  - `update` — Updates a customer.
  - `chrome` — Operations on the 'chrome' resource

### domainAliases

  - `delete` — Deletes a domain Alias of the customer.
  - `get` — Retrieves a domain alias of the customer.
  - `insert` — Inserts a domain alias of the customer.
  - `list` — Lists the domain aliases of the customer.

### domains

  - `delete` — Deletes a domain of the customer.
  - `get` — Retrieves a domain of the customer.
  - `insert` — Inserts a domain of the customer.
  - `list` — Lists the domains of the customer.

### groups

  - `delete` — Deletes a group.
  - `get` — Retrieves a group's properties.
  - `insert` — Creates a group.
  - `list` — Retrieves all groups of a domain or of a user given a userKey (paginated).
  - `patch` — Updates a group's properties. This method supports [patch semantics](https://developers.google.com/workspace/admin/directory/v1/guides/performance#patch).
  - `update` — Updates a group's properties.
  - `aliases` — Operations on the 'aliases' resource

### members

  - `delete` — Removes a member from a group.
  - `get` — Retrieves a group member's properties.
  - `hasMember` — Checks whether the given user is a member of the group. Membership can be direct or nested, but if nested, the `memberKey` and `groupKey` must be entities in the same domain or an `Invalid input` erro
  - `insert` — Adds a user to the specified group.
  - `list` — Retrieves a paginated list of all members in a group. This method times out after 60 minutes. For more information, see [Troubleshoot error codes](https://developers.google.com/workspace/admin/directo
  - `patch` — Updates the membership properties of a user in the specified group. This method supports [patch semantics](https://developers.google.com/workspace/admin/directory/v1/guides/performance#patch).
  - `update` — Updates the membership of a user in the specified group.

### mobiledevices

  - `action` — Takes an action that affects a mobile device. For example, remotely wiping a device.
  - `delete` — Removes a mobile device.
  - `get` — Retrieves a mobile device's properties.
  - `list` — Retrieves a paginated list of all user-owned mobile devices for an account. To retrieve a list that includes company-owned devices, use the Cloud Identity [Devices API](https://cloud.google.com/identi

### orgunits

  - `delete` — Removes an organizational unit.
  - `get` — Retrieves an organizational unit.
  - `insert` — Adds an organizational unit.
  - `list` — Retrieves a list of all organizational units for an account.
  - `patch` — Updates an organizational unit. This method supports [patch semantics](https://developers.google.com/workspace/admin/directory/v1/guides/performance#patch)
  - `update` — Updates an organizational unit.

### privileges

  - `list` — Retrieves a paginated list of all privileges for a customer.

### resources

  - `buildings` — Operations on the 'buildings' resource
  - `calendars` — Operations on the 'calendars' resource
  - `features` — Operations on the 'features' resource

### roleAssignments

  - `delete` — Deletes a role assignment.
  - `get` — Retrieves a role assignment.
  - `insert` — Creates a role assignment.
  - `list` — Retrieves a paginated list of all roleAssignments.

### roles

  - `delete` — Deletes a role.
  - `get` — Retrieves a role.
  - `insert` — Creates a role.
  - `list` — Retrieves a paginated list of all the roles in a domain.
  - `patch` — Patches a role.
  - `update` — Updates a role.

### schemas

  - `delete` — Deletes a schema.
  - `get` — Retrieves a schema.
  - `insert` — Creates a schema.
  - `list` — Retrieves all schemas for a customer.
  - `patch` — Patches a schema.
  - `update` — Updates a schema.

### tokens

  - `delete` — Deletes all access tokens issued by a user for an application.
  - `get` — Gets information about an access token issued by a user.
  - `list` — Returns the set of tokens specified user has issued to 3rd party applications.

### twoStepVerification

  - `turnOff` — Turns off 2-Step Verification for user.

### users

  - `createGuest` — Create a guest user with access to a [subset of Workspace capabilities](https://support.google.com/a/answer/16558545?hl=en). This feature is currently in Alpha. Please reach out to support if you are 
  - `delete` — Deletes a user.
  - `get` — Retrieves a user.
  - `insert` — Creates a user. Mutate calls immediately following user creation might sometimes fail as the user isn't fully created due to propagation delay in our backends. Check the error details for the "User cr
  - `list` — Retrieves a paginated list of either deleted users or all users in a domain.
  - `makeAdmin` — Makes a user a super administrator.
  - `patch` — Updates a user using patch semantics. The update method should be used instead, because it also supports patch semantics and has better performance. If you're mapping an external identity to a Google 
  - `signOut` — Signs a user out of all web and device sessions and reset their sign-in cookies. User will have to sign in by authenticating again.
  - `undelete` — Undeletes a deleted user.
  - `update` — Updates a user. This method supports patch semantics, meaning that you only need to include the fields you wish to update. Fields that are not present in the request will be preserved, and fields set 
  - `watch` — Watches for changes in users list.
  - `aliases` — Operations on the 'aliases' resource
  - `photos` — Operations on the 'photos' resource

### verificationCodes

  - `generate` — Generates new backup verification codes for the user.
  - `invalidate` — Invalidates the current backup verification codes for the user.
  - `list` — Returns the current set of valid backup verification codes for the specified user.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws admin --help

# Inspect a method's required params, types, and defaults
gws schema admin.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


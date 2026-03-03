---
name: gws-cloudidentity
version: 1.0.0
description: "Google Cloud Identity: Manage identity groups and memberships."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws cloudidentity --help"
---

# cloudidentity (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws cloudidentity <resource> <method> [flags]
```

## API Resources

### customers

  - `userinvitations` — Operations on the 'userinvitations' resource

### devices

  - `cancelWipe` — Cancels an unfinished device wipe. This operation can be used to cancel device wipe in the gap between the wipe operation returning success and the device being wiped. This operation is possible when 
  - `create` — Creates a device. Only company-owned device may be created. **Note**: This method is available only to customers who have one of the following SKUs: Enterprise Standard, Enterprise Plus, Enterprise fo
  - `delete` — Deletes the specified device.
  - `get` — Retrieves the specified device.
  - `list` — Lists/Searches devices.
  - `wipe` — Wipes all data on the specified device.
  - `deviceUsers` — Operations on the 'deviceUsers' resource

### groups

  - `create` — Creates a Group.
  - `delete` — Deletes a `Group`.
  - `get` — Retrieves a `Group`.
  - `getSecuritySettings` — Get Security Settings
  - `list` — Lists the `Group` resources under a customer or namespace.
  - `lookup` — Looks up the [resource name](https://cloud.google.com/apis/design/resource_names) of a `Group` by its `EntityKey`.
  - `patch` — Updates a `Group`.
  - `search` — Searches for `Group` resources matching a specified query.
  - `updateSecuritySettings` — Update Security Settings
  - `memberships` — Operations on the 'memberships' resource

### inboundOidcSsoProfiles

  - `create` — Creates an InboundOidcSsoProfile for a customer. When the target customer has enabled [Multi-party approval for sensitive actions](https://support.google.com/a/answer/13790448), the `Operation` in the
  - `delete` — Deletes an InboundOidcSsoProfile.
  - `get` — Gets an InboundOidcSsoProfile.
  - `list` — Lists InboundOidcSsoProfile objects for a Google enterprise customer.
  - `patch` — Updates an InboundOidcSsoProfile. When the target customer has enabled [Multi-party approval for sensitive actions](https://support.google.com/a/answer/13790448), the `Operation` in the response will 

### inboundSamlSsoProfiles

  - `create` — Creates an InboundSamlSsoProfile for a customer. When the target customer has enabled [Multi-party approval for sensitive actions](https://support.google.com/a/answer/13790448), the `Operation` in the
  - `delete` — Deletes an InboundSamlSsoProfile.
  - `get` — Gets an InboundSamlSsoProfile.
  - `list` — Lists InboundSamlSsoProfiles for a customer.
  - `patch` — Updates an InboundSamlSsoProfile. When the target customer has enabled [Multi-party approval for sensitive actions](https://support.google.com/a/answer/13790448), the `Operation` in the response will 
  - `idpCredentials` — Operations on the 'idpCredentials' resource

### inboundSsoAssignments

  - `create` — Creates an InboundSsoAssignment for users and devices in a `Customer` under a given `Group` or `OrgUnit`.
  - `delete` — Deletes an InboundSsoAssignment. To disable SSO, Create (or Update) an assignment that has `sso_mode` == `SSO_OFF`.
  - `get` — Gets an InboundSsoAssignment.
  - `list` — Lists the InboundSsoAssignments for a `Customer`.
  - `patch` — Updates an InboundSsoAssignment. The body of this request is the `inbound_sso_assignment` field and the `update_mask` is relative to that. For example: a PATCH to `/v1/inboundSsoAssignments/0abcdefg12

### policies

  - `get` — Get a policy.
  - `list` — List policies.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws cloudidentity --help

# Inspect a method's required params, types, and defaults
gws schema cloudidentity.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


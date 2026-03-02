---
name: gws-chat
version: 1.0.0
description: "USE WHEN the user wants to manage chat spaces and messages via the `gws` CLI."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws chat --help"
---

# chat (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws chat <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+send`](../gws-chat-send/SKILL.md) | Send a message to a space |

## API Resources

### customEmojis

  - `create` — Creates a custom emoji. Custom emojis are only available for Google Workspace accounts, and the administrator must turn custom emojis on for the organization. For more information, see [Learn about cu
  - `delete` — Deletes a custom emoji. By default, users can only delete custom emoji they created. [Emoji managers](https://support.google.com/a/answer/12850085) assigned by the administrator can delete any custom 
  - `get` — Returns details about a custom emoji. Custom emojis are only available for Google Workspace accounts, and the administrator must turn custom emojis on for the organization. For more information, see [
  - `list` — Lists custom emojis visible to the authenticated user. Custom emojis are only available for Google Workspace accounts, and the administrator must turn custom emojis on for the organization. For more i

### media

  - `download` — Downloads media. Download is supported on the URI `/v1/media/{+name}?alt=media`.
  - `upload` — Uploads an attachment. For an example, see [Upload media as a file attachment](https://developers.google.com/workspace/chat/upload-media-attachments). Requires user [authentication](https://developers

### spaces

  - `completeImport` — Completes the [import process](https://developers.google.com/workspace/chat/import-data) for the specified space and makes it visible to users. Requires [user authentication](https://developers.google
  - `create` — Creates a space. Can be used to create a named space, or a group chat in `Import mode`. For an example, see [Create a space](https://developers.google.com/workspace/chat/create-spaces). Supports the f
  - `delete` — Deletes a named space. Always performs a cascading delete, which means that the space's child resources—like messages posted in the space and memberships in the space—are also deleted. For an example,
  - `findDirectMessage` — Returns the existing direct message with the specified user. If no direct message space is found, returns a `404 NOT_FOUND` error. For an example, see [Find a direct message](/chat/api/guides/v1/space
  - `get` — Returns details about a space. For an example, see [Get details about a space](https://developers.google.com/workspace/chat/get-spaces). Supports the following types of [authentication](https://develo
  - `list` — Lists spaces the caller is a member of. Group chats and DMs aren't listed until the first message is sent. For an example, see [List spaces](https://developers.google.com/workspace/chat/list-spaces). 
  - `patch` — Updates a space. For an example, see [Update a space](https://developers.google.com/workspace/chat/update-spaces). If you're updating the `displayName` field and receive the error message `ALREADY_EXI
  - `search` — Returns a list of spaces in a Google Workspace organization based on an administrator's search. In the request, set `use_admin_access` to `true`. For an example, see [Search for and manage spaces](htt
  - `setup` — Creates a space and adds specified users to it. The calling user is automatically added to the space, and shouldn't be specified as a membership in the request. For an example, see [Set up a space wit
  - `members` — Operations on the 'members' resource
  - `messages` — Operations on the 'messages' resource
  - `spaceEvents` — Operations on the 'spaceEvents' resource

### users

  - `spaces` — Operations on the 'spaces' resource

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws chat --help

# Inspect a method's required params, types, and defaults
gws schema chat.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


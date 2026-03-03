---
name: gws-apps-script
version: 1.0.0
description: "Google Apps Script: Manage and execute Apps Script projects."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws apps-script --help"
---

# apps-script (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws apps-script <resource> <method> [flags]
```

## Helper Commands

| Command | Description |
|---------|-------------|
| [`+push`](../gws-apps-script-push/SKILL.md) | Upload local files to an Apps Script project |

## API Resources

### processes

  - `list` — List information about processes made by or on behalf of a user, such as process type and current status.
  - `listScriptProcesses` — List information about a script's executed processes, such as process type and current status.

### projects

  - `create` — Creates a new, empty script project with no script files and a base manifest file.
  - `get` — Gets a script project's metadata.
  - `getContent` — Gets the content of the script project, including the code source and metadata for each script file.
  - `getMetrics` — Get metrics data for scripts, such as number of executions and active users.
  - `updateContent` — Updates the content of the specified script project. This content is stored as the HEAD version, and is used when the script is executed as a trigger, in the script editor, in add-on preview mode, or 
  - `deployments` — Operations on the 'deployments' resource
  - `versions` — Operations on the 'versions' resource

### scripts

  - `run` — 

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws apps-script --help

# Inspect a method's required params, types, and defaults
gws schema apps-script.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


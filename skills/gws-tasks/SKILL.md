---
name: gws-tasks
version: 1.0.0
description: "Manages Google Tasks task lists and individual tasks via the gws CLI. Use when the user wants to create tasks, add items to a to-do list, set due dates, mark tasks complete, check off items, clear completed tasks, move or reorganize tasks, delete tasks, or manage Google Tasks task lists (insert, update, list, delete). Trigger terms: Google Tasks, todo, to-do, checklist, task reminder, add task, task list, pending items."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws tasks --help"
---

# tasks (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws tasks <resource> <method> [flags]
```

## API Resources

### tasklists

  - `delete` — Deletes the authenticated user's specified task list. If the list contains assigned tasks, both the assigned tasks and the original tasks in the assignment surface (Docs, Chat Spaces) are deleted.
  - `get` — Returns the authenticated user's specified task list.
  - `insert` — Creates a new task list and adds it to the authenticated user's task lists. A user can have up to 2000 lists at a time.
  - `list` — Returns all the authenticated user's task lists. A user can have up to 2000 lists at a time.
  - `patch` — Updates the authenticated user's specified task list. This method supports patch semantics.
  - `update` — Updates the authenticated user's specified task list.

### tasks

  - `clear` — Clears all completed tasks from the specified task list. The affected tasks will be marked as 'hidden' and no longer be returned by default when retrieving all tasks for a task list.
  - `delete` — Deletes the specified task from the task list. If the task is assigned, both the assigned task and the original task (in Docs, Chat Spaces) are deleted. To delete the assigned task only, navigate to the assignment surface and unassign the task from there.
  - `get` — Returns the specified task.
  - `insert` — Creates a new task on the specified task list. Tasks assigned from Docs or Chat Spaces cannot be inserted from Tasks Public API; they can only be created by assigning them from Docs or Chat Spaces. A user can have up to 20,000 non-hidden tasks per list and up to 100,000 tasks in total at a time.
  - `list` — Returns all tasks in the specified task list. Doesn't return assigned tasks by default (from Docs, Chat Spaces). A user can have up to 20,000 non-hidden tasks per list and up to 100,000 tasks in total at a time.
  - `move` — Moves the specified task to another position in the destination task list. If the destination list is not specified, the task is moved within its current list. This can include putting it as a child task under a new parent and/or move it to a different position among its sibling tasks. A user can have up to 2,000 subtasks per task.
  - `patch` — Updates the specified task. This method supports patch semantics.
  - `update` — Updates the specified task.

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws tasks --help

# Inspect a method's required params, types, and defaults
gws schema tasks.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.

## Examples

**Create a new task list:**
```bash
gws tasks tasklists insert --json '{"title": "Shopping List"}'
```

**Add a task with a due date to a task list:**
```bash
gws tasks tasks insert --params 'tasklist=<tasklistId>' \
  --json '{"title": "Buy groceries", "due": "2024-12-31T00:00:00.000Z"}'
```

**List all tasks in a task list:**
```bash
gws tasks tasks list --params 'tasklist=<tasklistId>'
```

## ⚠️ Destructive Operations

The following operations permanently affect tasks and cannot be undone — confirm intent before proceeding:

- **`tasks clear`** — Permanently hides all completed tasks from the specified list; they will no longer appear in default listings.
- **`tasks delete`** — Permanently deletes the specified task. If the task is assigned (from Docs or Chat Spaces), the original task in the assignment surface is also deleted.
- **`tasklists delete`** — Permanently deletes the entire task list and all assigned tasks within it.

**Verify before executing** — always fetch the resource first to confirm the correct IDs:

```bash
# Confirm the correct task before deleting
gws tasks tasks get --params 'tasklist=<tasklistId>,task=<taskId>'

# Confirm the correct task list before deleting
gws tasks tasklists get --params 'tasklist=<tasklistId>'

# Review completed tasks before clearing
gws tasks tasks list --params 'tasklist=<tasklistId>,showCompleted=true,showHidden=false'
```

Only proceed with the destructive command once the returned resource matches the user's intent.

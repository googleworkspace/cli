---
name: recipe-create-task-list
version: 1.0.0
description: "Creates a new Google Tasks list and populates it with initial tasks, including setting titles, notes, and due dates. Use when the user wants to set up a task list, create a to-do list, add tasks in Google Tasks, organize tasks, create a new todo, set up tasks in Google, or build a task manager list from scratch."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-tasks"]
---

# Create a Task List and Add Tasks

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-tasks`

Set up a new Google Tasks list with initial tasks.

## Steps

1. Create task list: `gws tasks tasklists insert --json '{"title": "Q2 Goals"}'`
   - Note the `id` field from the response — this is your `TASKLIST_ID` and is required for all subsequent steps.
2. Verify the list was created successfully by confirming the response contains a valid `id` before proceeding.
3. Add a task: `gws tasks tasks insert --params '{"tasklist": "TASKLIST_ID"}' --json '{"title": "Review Q1 metrics", "notes": "Pull data from analytics dashboard", "due": "2024-04-01T00:00:00Z"}'`
4. Add another task: `gws tasks tasks insert --params '{"tasklist": "TASKLIST_ID"}' --json '{"title": "Draft Q2 OKRs"}'`
5. List tasks: `gws tasks tasks list --params '{"tasklist": "TASKLIST_ID"}' --format table`

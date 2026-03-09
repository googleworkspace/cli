---
name: recipe-review-overdue-tasks
version: 1.0.0
description: "Lists, reviews, reschedules, and marks complete Google Tasks that are past due. Use when the user asks about overdue tasks, late Google Tasks, missed deadlines, incomplete todos, or wants to list, review, prioritize, reschedule, or mark complete tasks with passed due dates in Google Tasks."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-tasks"]
---

# Review Overdue Tasks

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-tasks`

Find Google Tasks that are past due and need attention.

## Steps

1. List task lists: `gws tasks tasklists list --format table`
2. List incomplete tasks for each relevant task list: `gws tasks tasks list --params '{"tasklist": "TASKLIST_ID", "showCompleted": false}' --format table`
3. Identify overdue items by comparing each task's `due` date against today's date — any task whose `due` date is earlier than the current date is overdue.
4. For each overdue task, take a concrete action as appropriate:
   - **Reschedule** — update the due date: `gws tasks tasks patch --params '{"tasklist": "TASKLIST_ID", "task": "TASK_ID", "due": "NEW_DATE_RFC3339"}'`
   - **Mark complete** — close the task: `gws tasks tasks patch --params '{"tasklist": "TASKLIST_ID", "task": "TASK_ID", "status": "completed"}'`
   - **Report** — summarise overdue tasks to the user with their titles, original due dates, and suggested next actions if no explicit action was requested.
5. After any patch operation, verify the update succeeded by re-listing the affected task: `gws tasks tasks get --params '{"tasklist": "TASKLIST_ID", "task": "TASK_ID"}'` and confirming the returned fields reflect the change. If the API returns an error, check for common causes:
   - **Invalid task or tasklist ID** — re-run the list commands to obtain fresh IDs.
   - **Permission denied** — confirm the authenticated account has write access to the task list.
   - **Invalid date format** — ensure `due` values are valid RFC 3339 timestamps (e.g., `2025-06-01T00:00:00Z`).

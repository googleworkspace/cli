---
name: recipe-daily-standup
version: 1.0.0
description: "USE WHEN the user wants a quick morning briefing of today's schedule and tasks."
metadata:
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins: ["gws"]
      skills: ["gws-workflow"]
---

# Run a Daily Standup Report

> **PREREQUISITE:** Load the following skills to execute this recipe: `gws-workflow`

USE WHEN the user wants a quick morning briefing of today's schedule and tasks.

## Steps

1. Generate standup report: `gws workflow +standup-report`
2. For table format: `gws workflow +standup-report --format table`
3. Share in team Chat if needed: Copy output and send via `gws chat spaces messages create`


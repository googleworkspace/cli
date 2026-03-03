---
"@googleworkspace/cli": minor
---

Add workflow helpers, personas, and 50 consumer-focused recipes

- Add `gws workflow` subcommand with 5 built-in helpers: `+standup-report`, `+meeting-prep`, `+email-to-task`, `+weekly-digest`, `+file-announce`
- Add 10 agent personas (exec-assistant, project-manager, sales-ops, etc.) with curated skill sets
- Add `docs/skills.md` skills index and `registry/recipes.yaml` with 50 multi-step recipes for Gmail, Drive, Docs, Calendar, and Sheets
- Update README with skills index link and accurate skill count
- Fix lefthook pre-commit to run fmt and clippy sequentially

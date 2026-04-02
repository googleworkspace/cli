---
name: gws-docs-suggest
description: "Google Docs: Create a tracked suggestion via browser automation."
metadata:
  version: 0.22.5
  openclaw:
    category: "productivity"
    requires:
      bins:
        - gws
        - node
    cliHelp: "gws docs +suggest --help"
---

# docs +suggest

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

Create a tracked suggestion in a Google Doc using browser automation.

The Google Docs API v1 has **no method to create suggestions** — all API writes
are direct edits. This command works around that limitation by launching a
headless browser via [Playwright](https://playwright.dev), switching the editor
to Suggesting mode, and performing a Find & Replace so the change appears as a
suggestion that collaborators can accept or reject.

See: https://issuetracker.google.com/issues/36054544

## Setup (one-time)

```bash
# Install Playwright and its Chromium browser
npx playwright install chromium

# Save a browser session with your Google credentials
npx playwright codegen --save-storage=state.json docs.google.com
# Log in in the browser that opens, then close it.
# Move the state to: ~/.config/gws/playwright-state.json
```

## Usage

```bash
gws docs +suggest --document <ID> --find <TEXT> --replace <TEXT> [--state-file <PATH>]
```

## Flags

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--document` | yes | — | Document ID |
| `--find` | yes | — | Exact text to find (must match exactly once) |
| `--replace` | yes | — | Replacement text (recorded as a suggestion) |
| `--state-file` | — | `~/.config/gws/playwright-state.json` | Path to Playwright browser state |

## Examples

```bash
# Suggest replacing a word
gws docs +suggest --document 1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgVE2upms \
  --find 'old paragraph text' \
  --replace 'new paragraph text'
```

## How it works

1. Launches headless Chromium with saved session cookies
2. Opens the document in the Google Docs editor
3. Switches from Editing to **Suggesting** mode via the toolbar
4. Opens Find & Replace (`Ctrl+H`)
5. Searches for the `--find` text and validates exactly one match exists
6. Clicks Replace — the change is recorded as a tracked suggestion
7. Closes the browser and saves the session

## Tips

- The `--find` text must match **exactly once** in the document. If multiple
  matches exist, use a longer quote to disambiguate.
- Each invocation takes ~15-30 seconds due to browser startup and page load.
- The browser session expires periodically. If you get auth errors, re-run
  `npx playwright codegen --save-storage=state.json docs.google.com` to refresh it.

> [!CAUTION]
> This is a **write** command — confirm with the user before executing.

## See Also

- [gws-docs-write](../gws-docs-write/SKILL.md) — Append text directly (no suggestions)
- [gws-shared](../gws-shared/SKILL.md) — Global flags and auth

---
description: Writing and editing VHS `.tape` files for terminal demo GIFs
---

# VHS Tape Files

[VHS](https://github.com/charmbracelet/vhs) records terminal sessions into GIFs/MP4s/WebMs from `.tape` scripts. Run with `vhs demo.tape`.

## Critical Syntax Rules

### Type command and inline directives

`Type`, `Sleep`, `Enter` are **separate directives on the same line**, delimited by the closing `"` of the `Type` string. The most common bug is forgetting to close the `Type` string, which causes `Sleep`/`Enter` to be typed literally into the terminal.

```
# ✅ CORRECT — closing " before Sleep
Type "echo hello" Sleep 300ms Enter

# ❌ WRONG — Sleep and Enter are typed as literal text
Type "echo hello Sleep 300ms Enter
```

### Type with @speed override

Override typing speed per-command with `@<time>` immediately after `Type` (no space):

```
Type@80ms '{"pageSize": 2}' Sleep 100ms
```

### Quoting

- Double quotes `"..."` are the standard Type delimiter
- Single quotes `'...'` also work and are useful when the typed content contains double quotes (e.g. JSON)
- Escape quotes inside strings with backticks: `` Type `VAR="value"` ``
- When building shell commands with nested quotes, split across multiple `Type` lines:

```
Type "gws drive files list --params '" Sleep 100ms
Type@80ms '{"pageSize": 2, "fields": "nextPageToken,files(id)"}' Sleep 100ms
Type "' --page-all" Sleep 300ms Enter
```

> **Pitfall**: Every `Type` line that is followed by `Sleep` or `Enter` on the same line MUST close its string first. Audit each line to ensure the quote is closed before any directive.

## Settings (top of file only)

Settings must appear before any non-setting command (except `Output`). `TypingSpeed` is the only setting that can be changed mid-tape.

```
Output demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1200
Set Height 1200
Set Theme "Catppuccin Mocha"
Set WindowBar Colorful
Set WindowBarSize 40
Set TypingSpeed 40ms
Set Padding 20
```

## Common Commands

| Command | Example | Notes |
|---|---|---|
| `Output` | `Output demo.gif` | `.gif`, `.mp4`, `.webm` |
| `Type` | `Type "ls -la"` | Type characters |
| `Type@<time>` | `Type@80ms "slow"` | Override typing speed |
| `Sleep` | `Sleep 2s`, `Sleep 300ms` | Pause recording |
| `Enter` | `Enter` | Press enter |
| `Hide` / `Show` | `Hide` ... `Show` | Hide setup commands |
| `Ctrl+<key>` | `Ctrl+C` | Key combos |
| `Tab`, `Space`, `Backspace` | `Tab 2` | Optional repeat count |
| `Up`, `Down`, `Left`, `Right` | `Up 3` | Arrow keys |
| `Wait` | `Wait /pattern/` | Wait for regex on screen |
| `Screenshot` | `Screenshot out.png` | Capture frame |
| `Env` | `Env FOO "bar"` | Set env var |
| `Source` | `Source other.tape` | Include another tape |
| `Require` | `Require jq` | Assert program exists |

## Hide/Show for Setup

Use `Hide`/`Show` to run setup commands (e.g. setting `$PATH`, clearing screen) without recording them:

```
Hide
Type "export PATH=$PWD/target/release:$PATH" Enter
Type "clear" Enter
Sleep 2s
Show
```

## Checklist When Editing Tape Files

1. **Every `Type` string must be closed** before `Sleep`/`Enter` on the same line
2. **Multi-line Type sequences** that build a single shell command: ensure the final line closes its string and includes `Enter`
3. **Sleep durations** after commands should be long enough for the command to finish (network calls may need 8s+)
4. **Settings go at the top** — only `TypingSpeed` can appear later
5. **Test locally** with `vhs <file>.tape` before committing

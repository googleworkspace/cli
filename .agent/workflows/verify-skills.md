---
description: Verify all skills/*/SKILL.md files against actual CLI output for accuracy
---

# Verify Skills

Ensure every `skills/*/SKILL.md` file is accurate and optimized for AI agent consumption.

## Steps

1. **List all skill files**

```bash
find skills -name SKILL.md | sort
```

2. **Get top-level help for every service**

// turbo
```bash
for svc in drive sheets gmail calendar admin admin-reports docs slides tasks people chat vault groupssettings reseller licensing apps-script; do
  echo "=== $svc ==="
  ./target/debug/gws $svc --help 2>&1
  echo
done
```

3. **Get sub-resource help for key services** (spot-check method names used in examples)

// turbo
```bash
./target/debug/gws drive files --help 2>&1
./target/debug/gws gmail users messages --help 2>&1
./target/debug/gws sheets spreadsheets --help 2>&1
./target/debug/gws sheets spreadsheets values --help 2>&1
./target/debug/gws calendar events --help 2>&1
./target/debug/gws people people --help 2>&1
./target/debug/gws chat spaces --help 2>&1
./target/debug/gws vault matters --help 2>&1
./target/debug/gws admin users --help 2>&1
./target/debug/gws tasks tasks --help 2>&1
```

4. **For each SKILL.md, verify the following against the CLI `--help` output:**

   - [ ] **Resource names** match exactly (e.g., `files`, `spreadsheets`, `users`)
   - [ ] **Method names** match exactly (e.g., `list`, `insert`, `batchUpdate`, `getContent`)
   - [ ] **Nested resource paths** are correct (e.g., `spreadsheets values get`, not `values get`)
   - [ ] **Alias** mentioned in the file matches `services.rs` (e.g., `gws script` for apps-script)
   - [ ] **API version** in the header is correct
   - [ ] **Example commands** use valid `--params` and `--json` flag syntax
   - [ ] **No OAuth scopes section** — scopes should not be listed in skill files
   - [ ] **Tips section** contains accurate, actionable advice

5. **Cross-check `shared/SKILL.md`** covers:

   - [ ] `--fields` / field mask syntax
   - [ ] CLI syntax (`--params`, `--json`, `--output`, `--upload`, `--page-all`, `--page-limit`, `--page-delay`)
   - [ ] Authentication (`GOOGLE_WORKSPACE_CLI_CREDENTIALS`, `GOOGLE_WORKSPACE_API_KEY`)
   - [ ] Auto-pagination (`--page-all`) with NDJSON output
   - [ ] `gws schema <method>` introspection
   - [ ] Error handling JSON structure
   - [ ] Binary download with `--output`
   - [ ] Version override (`--api-version`, colon syntax)

6. **Fix any issues found** — update the SKILL.md files directly.

7. **Rebuild and re-verify** if any examples were changed.

// turbo
```bash
cargo build 2>&1
```

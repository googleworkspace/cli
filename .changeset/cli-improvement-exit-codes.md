---
"@googleworkspace/cli": minor
---

Add semantic exit codes, structured error hints, and stderr/stdout separation for better CLI agent support

- Exit codes now reflect error type: 2 (usage), 3 (not found), 4 (auth), 5 (conflict), 75 (transient/retry), 78 (config)
- Error JSON includes new `transient` boolean and `fix` string fields for agent consumption
- Usage text prints to stderr on error paths so stdout stays machine-parseable
- Help text documents exit codes in the EXIT CODES section

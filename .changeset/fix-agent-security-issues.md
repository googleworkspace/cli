---
"@googleworkspace/cli": patch
---

Fix high-priority security and resource leak issues in agent tools:

- **Browser tool**: Close tabs after each operation to prevent memory accumulation during long agent sessions
- **GWS tool**: Fix recursion check to detect `agent` command regardless of preceding global flags, preventing infinite recursive loops
- **Supabase tool**: Remove comma and colon from URL encoding allow-list to prevent PostgREST query injection vulnerabilities

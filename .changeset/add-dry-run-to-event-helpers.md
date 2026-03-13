---
"@googleworkspace/cli": patch
---

feat(helpers): add --dry-run support to events helper commands

Add dry-run mode to `gws events +renew` and `gws events +subscribe` commands.
When --dry-run is specified, the commands will print what actions would be
taken without making any API calls. This allows agents to simulate requests
and learn without reaching the server.

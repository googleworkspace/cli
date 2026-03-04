---
"@googleworkspace/cli": minor
---

feat: support kebab-case aliases for camelCase subcommands (closes #32)

All API resource and method names that use camelCase are now also
accessible via their kebab-case equivalents.  Both forms are always
accepted, maintaining full backward compatibility.

**Examples**

Before (only form that worked):
```
gws gmail users getProfile --params '{"userId":"me"}'
gws calendar calendarList list
```

After (both forms work):
```
gws gmail users getProfile --params '{"userId":"me"}'   # still works
gws gmail users get-profile --params '{"userId":"me"}'  # new alias

gws calendar calendarList list                          # still works
gws calendar calendar-list list                         # new alias
```

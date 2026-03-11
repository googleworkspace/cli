---
"@googleworkspace/cli": patch
---

Validate `--subscription` and auto-generated slug names with `validate_resource_name()` in `gmail +watch` and `events +subscribe` to prevent path traversal and query injection via Pub/Sub resource names

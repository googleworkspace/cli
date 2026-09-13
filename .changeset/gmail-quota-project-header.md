---
"@googleworkspace/cli": patch
---

Fix Gmail helpers (`+read`, `+forward`, `+reply`, send-as resolution, original-attachment forwarding) returning a 403 accessNotConfigured error for ADC callers by adding the missing `x-goog-user-project` header

---
"@googleworkspace/cli": patch
---

fix: reject DEL character (0x7F) in input validation

The `reject_control_chars` helper rejected bytes 0x00–0x1F but allowed
the DEL character (0x7F), which is also an ASCII control character. This
could allow malformed input from LLM agents to bypass validation.

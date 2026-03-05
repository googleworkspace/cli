---
"@googleworkspace/cli": minor
---

Expose library crate (`lib.rs`) for programmatic API access. Extracts `config_dir()` and Model Armor sanitization types into standalone modules so they can be shared between the binary and library targets without pulling in CLI-only code.

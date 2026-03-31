---
"@googleworkspace/cli": patch
---

Migrated the internal AI skills registry (personas and recipes) from YAML to TOML. This allows us to drop the unmaintained serde_yaml dependency, improving the project's supply chain security posture.

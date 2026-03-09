---
"@googleworkspace/cli": patch
---

Stop persisting encryption key to `.encryption_key` file when OS keyring is available. Existing file-based keys are migrated into the keyring and the file is removed on next CLI invocation.

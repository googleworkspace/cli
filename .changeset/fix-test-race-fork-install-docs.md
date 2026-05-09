---
"@googleworkspace/cli": patch
---

fix(test): fix encryption key race in parallel tests; docs: add deb/rpm/Windows install sections to FORK.md. Replace raw set_var with EnvVarGuard in test_load_credentials_encrypted_file to prevent GOOGLE_WORKSPACE_CLI_CONFIG_DIR leaking into concurrent tests. Add serial attribute and per-test config-dir isolation to test_load_credentials_encrypted_takes_priority_over_default.

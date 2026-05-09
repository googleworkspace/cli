---
"@googleworkspace/cli": patch
---

fix(release): use -p crates/google-workspace-cli for cargo generate-rpm. The --manifest-path flag is not supported in cargo-generate-rpm 0.21.0; -p treats its argument as a directory path, so the correct form is -p crates/google-workspace-cli (which resolves to crates/google-workspace-cli/Cargo.toml).

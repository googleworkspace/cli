---
"@googleworkspace/cli": patch
---

Add crates.io publishing to release workflow

Publishes both `google-workspace` and `google-workspace-cli` to crates.io on each release. The library crate is published first (as a dependency), followed by the CLI crate.

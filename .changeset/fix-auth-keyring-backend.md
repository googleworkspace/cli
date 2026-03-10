---
"@googleworkspace/cli": patch
---

Fix `gws auth login` encrypted credential persistence by enabling native keyring backends for the `keyring` crate on supported desktop platforms instead of silently falling back to the in-memory mock store.

---
"@googleworkspace/cli": patch
---

Switch reqwest TLS from bundled Mozilla roots to native OS certificate store

This allows the CLI to trust custom or corporate CA certificates installed
in the system trust store, fixing TLS errors in enterprise environments.

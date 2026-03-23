---
"@googleworkspace/cli": minor
---

Refactor all `gws auth` subcommands to use clap for argument parsing

Replace manual argument parsing in `handle_auth_command`, `handle_login`, `resolve_scopes`, and `handle_export` with structured `clap::Command` definitions. Introduces `ScopeMode` enum for type-safe scope selection and adds proper `--help` support for all auth subcommands.

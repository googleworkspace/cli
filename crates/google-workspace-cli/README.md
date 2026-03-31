# google-workspace-cli

**One CLI for all of Google Workspace — built for humans and AI agents.**

`gws` dynamically generates its command surface at runtime by reading Google's [Discovery Service](https://developers.google.com/discovery). Drive, Gmail, Calendar, and every Workspace API — zero boilerplate, structured JSON output, 40+ agent skills included.

## Install

Download the pre-built binary for your OS and architecture from the **[GitHub Releases](https://github.com/googleworkspace/cli/releases)** page.

Alternatively, you can use package managers as a convenience layer:

```bash
npm install -g @googleworkspace/cli    # npm (downloads GitHub release binary)
cargo install google-workspace-cli     # crates.io
nix run github:googleworkspace/cli     # nix
```

## Quick Start

```bash
gws auth login
gws drive files list --params '{"pageSize": 5}'
gws gmail users.messages list --params '{"maxResults": 3}'
```

## Documentation

See the [full README](https://github.com/googleworkspace/cli#readme) for authentication setup, helper commands, agent skills, and more.

## License

Apache-2.0 — see [LICENSE](https://github.com/googleworkspace/cli/blob/main/LICENSE).

# How to contribute

We'd love to accept your patches and contributions to this project.

## Before you begin

### Sign our Contributor License Agreement

Contributions to this project must be accompanied by a
[Contributor License Agreement](https://cla.developers.google.com/about) (CLA).
You (or your employer) retain the copyright to your contribution; this simply
gives us permission to use and redistribute your contributions as part of the
project.

If you or your current employer have already signed the Google CLA (even if it
was for a different project), you probably don't need to do it again.

Visit <https://cla.developers.google.com/> to see your current agreements or to
sign a new one.

### Review our community guidelines

This project follows
[Google's Open Source Community Guidelines](https://opensource.google/conduct/).

## Contribution process

### Code reviews

All submissions, including submissions by project members, require review. We
use GitHub pull requests for this purpose. Consult
[GitHub Help](https://help.github.com/articles/about-pull-requests/) for more
information on using pull requests.

### Updating CI Smoketest Credentials

If the OAuth refresh token used in the GitHub Actions smoketest expires or needs additional scopes, you can generate a new one and update the repository secret using the GitHub CLI (`gh`).

1. **Set the credentials file path to output plaintext JSON**:
   ```bash
   export GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE=smoketest-creds.json
   ```

2. **Authenticate with the required scopes**:
   ```bash
   cargo run -- auth login --scopes https://www.googleapis.com/auth/drive,https://www.googleapis.com/auth/gmail.readonly,https://www.googleapis.com/auth/calendar.readonly,https://www.googleapis.com/auth/presentations.readonly,https://www.googleapis.com/auth/tasks.readonly
   ```

3. **Export and set the GitHub actions secret**:
   ```bash
   cargo run --quiet -- auth export --unmasked | base64 | gh secret set GOOGLE_CREDENTIALS_JSON
   ```

4. **Clean up**:
   ```bash
   rm smoketest-creds.json
   unset GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE
   ```

## Development Patterns

### Changesets

Every PR must include a changeset file at `.changeset/<descriptive-name>.md`:

```markdown
---
"@googleworkspace/cli": patch
---

Brief description of the change
```

Use `patch` for fixes/chores, `minor` for new features, `major` for breaking changes.

### Input Validation & URL Safety

This CLI is designed to be invoked by AI/LLM agents, so all user-supplied inputs must be treated as potentially adversarial. See [AGENTS.md](../AGENTS.md#input-validation--url-safety) for the full reference. The key rules are:

| What you're doing | What to use |
|---|---|
| Accepting a file path (`--output-dir`, `--dir`) | `validate::validate_safe_output_dir()` or `validate_safe_dir_path()` |
| Embedding a value in a URL path segment | `helpers::encode_path_segment()` |
| Passing query parameters | reqwest `.query()` builder (never string interpolation) |
| Using a resource name in a URL (`--project`, `--space`) | `helpers::validate_resource_name()` |
| Accepting an enum flag (`--msg-format`) | clap `value_parser` (see `gmail/mod.rs`) |

### Testing Expectations

- All new validation logic must include **both happy-path and error-path tests**
- Tests that modify the process CWD must use `#[serial]` from `serial_test`
- Tempdir paths should be canonicalized before use to handle macOS `/var` → `/private/var` symlinks
- Run the full suite before submitting: `cargo test && cargo clippy -- -D warnings`
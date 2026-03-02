# Helper Modules

This directory contains "Helper" implementations that provide high-value, simplified commands for complex Google Workspace API operations.

## Philosophy

The goal of the `gws` CLI is to provide raw access to the Google Workspace APIs. However, some operations are common but complex to execute via raw API calls (e.g., sending an email, appending a row to a sheet).

**Helper commands should only be added if they offer "High Usefulness":**

*   **Complex Abstraction:** Does it abstract away significant complexity (e.g., MIME encoding, complex JSON structures, multiple API calls)?
*   **Format Conversion:** Does it handle data format conversions that are tedious for the user?
*   **Not Just an Alias:** Avoid adding helpers that simply alias a single, straightforward API call.

## Architecture

Helpers are implemented using the `Helper` trait defined in `mod.rs`.

```rust
pub trait Helper: Send + Sync {
    fn inject_commands(
        &self,
        cmd: Command,
        doc: &crate::discovery::RestDescription,
    ) -> Command;

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>>;
}
```

*   **`inject_commands`**: Adds subcommands to the main service command. All helper commands are always shown regardless of authentication state.
*   **`handle`**: implementation of the command logic. Returns `Ok(true)` if the command was handled, or `Ok(false)` to let the default raw resource handler attempt to handle it.

### Catalogue

| Service | Command | Usage | Description | Equivalent Raw Command (Example) |
| :--- | :--- | :--- | :--- | :--- |
| **Gmail** | `+send` | `gws gmail +send ...` | Sends an email. | `gws gmail users messages send ...` |
| **Sheets** | `+append` | `gws sheets +append ...` | Appends a row. | `gws sheets spreadsheets values append ...` |
| **Sheets** | `+read` | `gws sheets +read ...` | Reads values. | `gws sheets spreadsheets values get ...` |
| **Docs** | `+write` | `gws docs +write ...` | Appends text. | `gws docs documents batchUpdate ...` |
| **Chat** | `+send` | `gws chat +send ...` | Sends message. | `gws chat spaces messages create ...` |
| **Drive** | `+upload` | `gws drive +upload ...` | Uploads file. | `gws drive files create --upload ...` |
| **Calendar** | `+insert` | `gws calendar +insert ...` | Creates event. | `gws calendar events insert ...` |
| **Script** | `+push` | `gws script +push --script <ID>` | Pushes files. | `gws script projects updateContent ...` |
| **Events** | `+subscribe` | `gws events +subscribe ...` | Subscribe & stream events. | Pub/Sub REST + Workspace Events API |
| **Events** | `+renew` | `gws events +renew ...` | Renew subscriptions. | `gws events subscriptions reactivate ...` |

### Development

To add a new helper:
1.  Create `src/helpers/<service>.rs`.
2.  Implement the `Helper` trait.
3.  Register it in `src/helpers/mod.rs`.
4.  **Prefix** the command with `+` (e.g., `+create`).

## Current Helpers

*   **Gmail**: Sending emails (abstracts RFC 2822 encoding).
*   **Sheets**: Appending rows (abstracts `ValueRange` JSON construction).
*   **Docs**: Appending text (abstracts `batchUpdate` requests).
*   **Chat**: Sending messages to spaces.

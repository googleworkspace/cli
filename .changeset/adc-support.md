---
"@googleworkspace/cli": minor
---

Add Application Default Credentials (ADC) support.

`gws` now discovers ADC as a fourth credential source, after the encrypted
and plaintext credential files. The lookup order is:

1. `GOOGLE_WORKSPACE_CLI_TOKEN` env var (raw access token, highest priority)
2. `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE` env var
3. Encrypted credentials (`~/.config/gws/credentials.enc`)
4. Plaintext credentials (`~/.config/gws/credentials.json`)
5. **ADC** — `GOOGLE_APPLICATION_CREDENTIALS` env var (hard error if file missing), then
   `~/.config/gcloud/application_default_credentials.json` (silent if absent)

This means `gcloud auth application-default login --client-id-file=client_secret.json`
is now a fully supported auth flow — no need to run `gws auth login` separately.
Both `authorized_user` and `service_account` ADC formats are supported.

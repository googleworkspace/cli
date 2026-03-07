---
"@googleworkspace/cli": patch
---

Harden URL and path construction across helper modules (fixes #87)

- `discovery.rs`: Add `validate_discovery_id()` that allows only alphanumerics,
  `-`, `_`, `.` for service names and version strings. Validate both before using
  them in the local cache file path (path-traversal prevention) or in Discovery
  Document URLs. Move the `version` query parameter in the alt-URL code path to
  `reqwest`'s `.query()` builder instead of string interpolation.

- `modelarmor.rs`: Call `validate_resource_name()` on the `--template` resource
  name in `handle_sanitize` and `build_sanitize_request_data` before embedding
  it in a URL. Validate `--project`, `--location`, and `--template-id` in
  `parse_create_template_args` before they reach the URL builder. Use
  `encode_path_segment()` to percent-encode `templateId` in the query string.

- `gmail/watch.rs`: Extract message-URL construction into a dedicated
  `build_message_url()` helper that uses `encode_path_segment()` on the message
  ID. Switch `msg_format` from string interpolation to reqwest's `.query()`
  builder.

Adds 15 new unit tests (happy-path + error-path) covering each fix.

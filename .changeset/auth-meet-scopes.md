---
"@googleworkspace/cli": patch
---

Fix `gws auth login -s meet` not granting any Meet scopes. The Meet API exposes scopes under the `meetings.*` prefix (e.g. `meetings.space.readonly`), but the CLI's service alias is `meet`, so the scope-picker filter matched nothing and silently dropped every Meet scope. Map the `meet` service to the `meetings` scope prefix so the existing dynamic-augmentation path pulls all Meet scopes from the Discovery document when `-s meet` is supplied.

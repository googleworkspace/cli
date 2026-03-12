---
"@googleworkspace/cli": minor
---

Add `--upload-content-type` flag and smart MIME inference for multipart uploads

Previously, multipart uploads used the metadata `mimeType` field for both the Drive
metadata and the media part's `Content-Type` header. This made it impossible to upload
a file in one format (e.g. Markdown) and have Drive convert it to another (e.g. Google Docs),
because the media `Content-Type` and the target `mimeType` must differ for import conversions.

The new `--upload-content-type` flag allows setting the media `Content-Type` explicitly.
When omitted, the media type is now inferred from the file extension before falling back
to the metadata `mimeType`. This matches Google Drive's model where metadata `mimeType`
is the *target* type (what the file should become) while the media `Content-Type` is the
*source* type (what the bytes are).

This means import conversions now work automatically:
```bash
# Extension inference detects text/markdown → conversion just works
gws drive files create \
  --json '{"name":"My Doc","mimeType":"application/vnd.google-apps.document"}' \
  --upload notes.md

# Explicit flag still available as an override
gws drive files create \
  --json '{"name":"My Doc","mimeType":"application/vnd.google-apps.document"}' \
  --upload notes.md \
  --upload-content-type text/markdown
```

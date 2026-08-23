---
"@googleworkspace/cli": patch
---

Fix `drive.files.download` not writing the file for large Shared-Drive downloads.

For a large binary file living in a Shared Drive, Drive can respond to a download
request with a `drive#operation` JSON envelope naming a `downloadUri` to fetch the
actual bytes from, instead of returning the bytes directly. The response handler had
no branch for this shape: it fell into the ordinary JSON-response path, printed the
envelope itself as if it were the command's output, and never wrote the requested
`--output` file at all — silently, with no error.

The download-response envelope is now recognized (`kind: "drive#operation"` carrying
a `downloadUri`/`downloadUrl`) and its URI is followed with a second request before
the file is written, restricted to Google's own API/storage/user-content hosts.

---
"@googleworkspace/cli": patch
---

Fix 204 No Content responses being mis-routed to the binary download path.

Endpoints whose 204 response happens to carry a non-empty `Content-Type` header
(e.g. `calendar events delete` returning `text/html`) were falling through to
`handle_binary_response()`, writing an empty `download.html` to cwd and emitting
a spurious status JSON to stdout.

Added an early `break` when `status == 204` so all No Content responses produce
clean empty output, matching the behaviour of other delete endpoints.

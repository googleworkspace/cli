---
"@googleworkspace/cli": minor
---

Add `--allow-unknown-fields` to raw API methods with JSON request bodies. Explicitly
allow fields absent from Discovery recursively, including in dry runs, while
preserving validation of known fields, required fields, JSON, URLs and file paths.
Handwritten helpers retain strict validation.

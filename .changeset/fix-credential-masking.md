---
"gws": patch
---

fix: credential masking panic and silent token write errors

Fixed `gws auth export` masking which panicked on short strings and showed
the entire secret instead of masking it. Also fixed silent token cache write
failures in `save_to_disk` that returned `Ok(())` even when the write failed.

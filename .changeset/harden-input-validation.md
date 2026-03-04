---
"@googleworkspace/cli": patch
---

fix: harden input validation for AI/LLM callers

- Add `src/validate.rs` with `validate_safe_output_dir`, `validate_msg_format`, and `validate_safe_dir_path` helpers
- Validate `--output-dir` against path traversal in `gmail +watch` and `events +subscribe`
- Validate `--msg-format` against allowlist (full, metadata, minimal, raw) in `gmail +watch`
- Validate `--dir` against path traversal in `script +push`
- Add clap `value_parser` constraint for `--msg-format`
- Document input validation patterns in `AGENTS.md`

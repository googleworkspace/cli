---
"@googleworkspace/cli": patch
---

Clean up nits from PR #175 auth fix

- Update stale docstring on `resolve_account` to match new fallthrough behavior
- Add breadcrumb comment on string-based error matching in `main.rs`
- Move identity scope injection before authenticator build for readability

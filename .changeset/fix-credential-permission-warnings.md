---
"@googleworkspace/cli": patch
---

fix: warn on credential file permission failures instead of ignoring

Replaced silent `let _ =` on `set_permissions` calls in `save_encrypted`
with `eprintln!` warnings so users are aware if their credential files
end up with insecure permissions. Also log keyring access failures
instead of silently falling through to file storage.

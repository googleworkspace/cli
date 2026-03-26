---
"@googleworkspace/cli": patch
---

feat(auth): use strict OS keychain integration on macOS and Windows

Closes #623. The CLI no longer writes a fallback `.encryption_key` text file on macOS and Windows when securely storing credentials. Instead, it strictly uses the native OS keychain (Keychain Access on macOS, Credential Manager on Windows). If an old `.encryption_key` file is found during a successful keychain login, it will be automatically deleted for security. 
Linux deployments continue to use a seamless file-based fallback by default to ensure maximum compatibility with headless continuous integration (CI) runners, Docker containers, and SSH environments without desktop DBUS services.

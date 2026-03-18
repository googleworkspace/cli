---
"@googleworkspace/cli": patch
---

Handle SIGTERM in `gws gmail +watch` and `gws events +subscribe` for clean container shutdown.

Long-running pull loops now exit gracefully on SIGTERM (in addition to Ctrl+C),
enabling clean shutdown under Kubernetes, Docker, and systemd.

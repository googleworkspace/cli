---
"@googleworkspace/cli": patch
---

Consolidate terminal sanitization, coloring, and output helpers into a new `output.rs` module. Fixes raw ANSI escape codes in `watch.rs` that bypassed `NO_COLOR` and TTY detection, upgrades `sanitize_for_terminal` to also strip dangerous Unicode characters (bidi overrides, zero-width spaces, directional isolates), and sanitizes previously raw API error body and user query outputs.

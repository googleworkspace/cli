---
"@googleworkspace/cli": patch
---

fix: use terminal-default colors for TUI text to fix readability on light themes

Previously, item labels in the scope picker, project picker, and step progress
list used `Color::White` for foreground text. On light-background terminals this
made the text nearly invisible against the light terminal background.

- Replace `Color::White` label fg with `Style::default()` so the terminal's own
  default foreground color (black on light themes, white on dark themes) is used.
- Replace `Color::Gray` for unselected item labels with `Color::DarkGray` for
  consistent dimming that remains readable on light terminals.
- Apply the same fix to the inline text input widget.

Fixes #139

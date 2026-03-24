// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Shared output helpers for terminal sanitization, coloring, and stderr
//! messaging.
//!
//! Every function that prints untrusted content to the terminal should use
//! these helpers to prevent escape-sequence injection, Unicode spoofing,
//! and to respect `NO_COLOR` / non-TTY environments.

// Import dangerous-char detection from the library crate.
pub(crate) use google_workspace::validate::is_dangerous_unicode;

// ── Sanitization ──────────────────────────────────────────────────────

/// Strip dangerous characters from untrusted text before printing to the
/// terminal.  Removes ASCII control characters (except `\n` and `\t`,
/// which are preserved for readability) and dangerous Unicode characters
/// (bidi overrides, zero-width chars, line/paragraph separators).
pub(crate) fn sanitize_for_terminal(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            if c == '\n' || c == '\t' {
                return true;
            }
            if c.is_control() {
                return false;
            }
            !is_dangerous_unicode(c)
        })
        .collect()
}

// ── Color ─────────────────────────────────────────────────────────────

/// Returns true when stderr is connected to an interactive terminal and
/// `NO_COLOR` is not set, meaning ANSI color codes will be visible.
pub(crate) fn stderr_supports_color() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Wrap `text` in ANSI bold + the given color code, resetting afterwards.
/// Returns the plain text unchanged when stderr is not a TTY or `NO_COLOR`
/// is set.
pub(crate) fn colorize(text: &str, ansi_color: &str) -> String {
    if stderr_supports_color() && ansi_color.chars().all(|c| c.is_ascii_digit()) {
        format!("\x1b[1;{ansi_color}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

// ── Stderr helpers ────────────────────────────────────────────────────

/// Print a status message to stderr. The message is sanitized before
/// printing to prevent terminal injection.
#[allow(dead_code)]
pub(crate) fn status(msg: &str) {
    eprintln!("{}", sanitize_for_terminal(msg));
}

/// Print a warning to stderr with a colored prefix. The message is
/// sanitized before printing.
#[allow(dead_code)]
pub(crate) fn warn(msg: &str) {
    let prefix = colorize("warning:", "33"); // yellow
    eprintln!("{prefix} {}", sanitize_for_terminal(msg));
}

/// Print an informational message to stderr. The message is sanitized
/// before printing.
#[allow(dead_code)]
pub(crate) fn info(msg: &str) {
    eprintln!("{}", sanitize_for_terminal(msg));
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_for_terminal ─────────────────────────────────────

    #[test]
    fn sanitize_strips_ansi_escape_sequences() {
        let input = "normal \x1b[31mred text\x1b[0m end";
        let sanitized = sanitize_for_terminal(input);
        assert_eq!(sanitized, "normal [31mred text[0m end");
        assert!(!sanitized.contains('\x1b'));
    }

    #[test]
    fn sanitize_preserves_newlines_and_tabs() {
        let input = "line1\nline2\ttab";
        assert_eq!(sanitize_for_terminal(input), "line1\nline2\ttab");
    }

    #[test]
    fn sanitize_strips_bell_and_backspace() {
        let input = "hello\x07bell\x08backspace";
        assert_eq!(sanitize_for_terminal(input), "hellobellbackspace");
    }

    #[test]
    fn sanitize_strips_carriage_return() {
        let input = "real\rfake";
        assert_eq!(sanitize_for_terminal(input), "realfake");
    }

    #[test]
    fn sanitize_strips_bidi_overrides() {
        let input = "hello\u{202E}dlrow";
        assert_eq!(sanitize_for_terminal(input), "hellodlrow");
    }

    #[test]
    fn sanitize_strips_zero_width_chars() {
        assert_eq!(sanitize_for_terminal("foo\u{200B}bar"), "foobar");
        assert_eq!(sanitize_for_terminal("foo\u{FEFF}bar"), "foobar");
    }

    #[test]
    fn sanitize_strips_line_separators() {
        assert_eq!(sanitize_for_terminal("line1\u{2028}line2"), "line1line2");
        assert_eq!(sanitize_for_terminal("para1\u{2029}para2"), "para1para2");
    }

    #[test]
    fn sanitize_strips_directional_isolates() {
        assert_eq!(sanitize_for_terminal("a\u{2066}b\u{2069}c"), "abc");
    }

    #[test]
    fn sanitize_preserves_normal_unicode() {
        assert_eq!(sanitize_for_terminal("日本語 café αβγ"), "日本語 café αβγ");
    }

    // ── colorize ──────────────────────────────────────────────────

    #[test]
    fn colorize_returns_text_in_no_color_mode() {
        let result = colorize("hello", "31");
        assert!(result.contains("hello"));
    }
}

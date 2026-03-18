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

use crate::error::GwsError;

// ── Dangerous character detection ─────────────────────────────────────

/// Returns `true` for Unicode characters that are dangerous in terminal
/// output but not caught by `char::is_control()`: zero-width chars, bidi
/// overrides, Unicode line/paragraph separators, and directional isolates.
///
/// Using `matches!` with char ranges gives O(1) per character instead of the
/// O(M) linear scan that a slice `.contains()` would require.
pub(crate) fn is_dangerous_unicode(c: char) -> bool {
    matches!(c,
        // zero-width: ZWSP, ZWNJ, ZWJ, BOM/ZWNBSP
        '\u{200B}'..='\u{200D}' | '\u{FEFF}' |
        // bidi: LRE, RLE, PDF, LRO, RLO
        '\u{202A}'..='\u{202E}' |
        // line / paragraph separators
        '\u{2028}'..='\u{2029}' |
        // directional isolates: LRI, RLI, FSI, PDI
        '\u{2066}'..='\u{2069}'
    )
}

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

/// Rejects strings containing control characters (C0: U+0000–U+001F,
/// C1: U+0080–U+009F, and DEL: U+007F) or dangerous Unicode characters
/// such as zero-width chars, bidi overrides, and line/paragraph separators.
///
/// Used for validating CLI argument values at the parse boundary.
pub(crate) fn reject_dangerous_chars(value: &str, flag_name: &str) -> Result<(), GwsError> {
    for c in value.chars() {
        if c.is_control() {
            return Err(GwsError::Validation(format!(
                "{flag_name} contains invalid control characters"
            )));
        }
        if is_dangerous_unicode(c) {
            return Err(GwsError::Validation(format!(
                "{flag_name} contains invalid Unicode characters"
            )));
        }
    }
    Ok(())
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

    // ── reject_dangerous_chars ────────────────────────────────────

    #[test]
    fn reject_clean_string() {
        assert!(reject_dangerous_chars("hello/world", "test").is_ok());
    }

    #[test]
    fn reject_tab() {
        assert!(reject_dangerous_chars("hello\tworld", "test").is_err());
    }

    #[test]
    fn reject_newline() {
        assert!(reject_dangerous_chars("hello\nworld", "test").is_err());
    }

    #[test]
    fn reject_del() {
        assert!(reject_dangerous_chars("hello\x7Fworld", "test").is_err());
    }

    #[test]
    fn reject_zero_width_space() {
        assert!(reject_dangerous_chars("foo\u{200B}bar", "test").is_err());
    }

    #[test]
    fn reject_bom() {
        assert!(reject_dangerous_chars("foo\u{FEFF}bar", "test").is_err());
    }

    #[test]
    fn reject_rtl_override() {
        assert!(reject_dangerous_chars("foo\u{202E}bar", "test").is_err());
    }

    #[test]
    fn reject_line_separator() {
        assert!(reject_dangerous_chars("foo\u{2028}bar", "test").is_err());
    }

    #[test]
    fn reject_paragraph_separator() {
        assert!(reject_dangerous_chars("foo\u{2029}bar", "test").is_err());
    }

    #[test]
    fn reject_zero_width_joiner() {
        assert!(reject_dangerous_chars("foo\u{200D}bar", "test").is_err());
    }

    #[test]
    fn reject_preserves_normal_unicode() {
        assert!(reject_dangerous_chars("日本語", "test").is_ok());
        assert!(reject_dangerous_chars("café", "test").is_ok());
        assert!(reject_dangerous_chars("αβγ", "test").is_ok());
    }

    #[test]
    fn reject_c1_control_csi() {
        // U+009B is the C1 "Control Sequence Introducer" — can inject
        // terminal escape sequences just like ESC+[
        assert!(reject_dangerous_chars("foo\u{009B}bar", "test").is_err());
    }

    // ── colorize ──────────────────────────────────────────────────

    #[test]
    fn colorize_returns_text_in_no_color_mode() {
        // In test environment, stderr is typically not a TTY
        let result = colorize("hello", "31");
        // Either plain text (no TTY) or colored (TTY) — we just verify
        // it contains the original text
        assert!(result.contains("hello"));
    }
}

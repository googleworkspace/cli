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

/// Max chars for CLI `--help` method descriptions (terminal-width friendly).
pub const CLI_DESCRIPTION_LIMIT: usize = 200;

/// Max chars for YAML frontmatter / skill index descriptions (compact metadata).
pub const FRONTMATTER_DESCRIPTION_LIMIT: usize = 120;

/// Max chars for skill body method descriptions (markdown, agents benefit from detail).
pub const SKILL_BODY_DESCRIPTION_LIMIT: usize = 500;

/// Truncates a description string to `max_chars` using smart boundaries.
///
/// When `strip_links` is true, markdown links `[text](url)` are replaced with
/// just `text` to reclaim character budget (useful for CLI help / frontmatter).
/// When false, links are preserved (useful for skill body text where agents can
/// follow URLs).
///
/// Truncation strategy:
/// 1. If a complete sentence (ending in `. `) fits within the limit, truncate there.
/// 2. Otherwise, break at the last word boundary (space) and append `…`.
/// 3. If no space exists, hard-cut at `max_chars - 1` and append `…`.
pub fn truncate_description(desc: &str, max_chars: usize, strip_links: bool) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let cleaned = if strip_links {
        strip_markdown_links(desc)
    } else {
        desc.to_string()
    };
    let trimmed = cleaned.trim();

    // Count chars (UTF-8 safe)
    let char_count = trimmed.chars().count();
    if char_count <= max_chars {
        return trimmed.to_string();
    }

    // Collect the first `max_chars` characters as a string to search within.
    let prefix: String = trimmed.chars().take(max_chars).collect();

    // Try to find the last complete sentence within the limit.
    // A sentence ends with ". " followed by more text, or "." at the end of
    // the prefix. We look for the last ". " to find a sentence boundary.
    if let Some(sentence_end) = find_last_sentence_boundary(&prefix) {
        let truncated: String = trimmed.chars().take(sentence_end).collect();
        return truncated;
    }

    // Fall back to last word boundary (space) within the limit.
    if let Some(last_space) = rfind_char_boundary(&prefix, ' ') {
        let truncated: String = trimmed.chars().take(last_space).collect();
        return format!("{truncated}…");
    }

    // Hard cut — no spaces at all
    let truncated: String = trimmed.chars().take(max_chars - 1).collect();
    format!("{truncated}…")
}

/// Strips markdown-style links `[text](url)` and replaces them with just `text`.
fn strip_markdown_links(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '[' {
            // Look for the closing ] followed by (
            if let Some(close_bracket) = find_char_from(&chars, ']', i + 1) {
                if close_bracket + 1 < len && chars[close_bracket + 1] == '(' {
                    if let Some(close_paren) = find_char_from(&chars, ')', close_bracket + 2) {
                        // Found a complete [text](url) — emit just the text
                        result.extend(&chars[i + 1..close_bracket]);
                        i = close_paren + 1;
                        continue;
                    }
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Finds the character-index of `target` starting from position `from`.
fn find_char_from(chars: &[char], target: char, from: usize) -> Option<usize> {
    chars[from..]
        .iter()
        .position(|&c| c == target)
        .map(|p| from + p)
}

/// Finds the last sentence boundary within a char-indexed string.
/// A sentence boundary is a position right after ". " where we can cleanly cut.
/// Returns the char-count to include (up to and including the period).
fn find_last_sentence_boundary(prefix: &str) -> Option<usize> {
    let chars: Vec<char> = prefix.chars().collect();
    let mut last_boundary = None;

    for (i, _) in chars.iter().enumerate() {
        if chars[i] == '.' {
            let after_period = i + 1;
            // Sentence boundary: period followed by a space, or period at end of prefix
            if after_period == chars.len()
                || (after_period < chars.len() && chars[after_period] == ' ')
            {
                last_boundary = Some(after_period);
            }
        }
    }

    last_boundary
}

/// Finds the last occurrence of `target` in a string, returning its char-index.
fn rfind_char_boundary(s: &str, target: char) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    chars.iter().rposition(|&c| c == target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_desc_unchanged() {
        let desc = "Lists all files.";
        assert_eq!(truncate_description(desc, 200, true), "Lists all files.");
    }

    #[test]
    fn truncate_at_sentence_boundary() {
        let desc = "Creates a file in Drive. This method supports multipart upload. See the guide for details on how to use it.";
        // At limit 30, only the first sentence fits before the sentence boundary.
        let result = truncate_description(desc, 30, true);
        assert_eq!(result, "Creates a file in Drive.");

        // At limit 70, both first and second sentences fit.
        let result = truncate_description(desc, 70, true);
        assert_eq!(
            result,
            "Creates a file in Drive. This method supports multipart upload."
        );
    }

    #[test]
    fn truncate_at_word_boundary() {
        let desc = "Create a guest user with access to a subset of Workspace capabilities";
        let result = truncate_description(desc, 50, true);
        // Should cut at the last space before char 50
        assert!(result.ends_with('…'));
        assert!(result.len() <= 55); // 50 chars + ellipsis
        assert!(!result.contains("capabil")); // Should not cut mid-word
    }

    #[test]
    fn hard_cut_no_spaces() {
        let desc = "abcdefghijklmnopqrstuvwxyz";
        let result = truncate_description(desc, 10, true);
        assert_eq!(result, "abcdefghi…");
    }

    #[test]
    fn strips_markdown_links() {
        let desc = "Create a guest user with access to a [subset of Workspace capabilities](https://support.google.com/a/answer/16558545). This feature is in Alpha.";
        let result = truncate_description(desc, 200, true);
        assert_eq!(
            result,
            "Create a guest user with access to a subset of Workspace capabilities. This feature is in Alpha."
        );
        assert!(!result.contains("https://"));
        assert!(!result.contains('['));
    }

    #[test]
    fn preserves_links_when_strip_links_false() {
        let desc = "Create a guest user with access to a [subset of Workspace capabilities](https://support.google.com/a/answer/16558545). This feature is in Alpha.";
        let result = truncate_description(desc, 500, false);
        assert!(result.contains("https://support.google.com"));
        assert!(result.contains("[subset of Workspace capabilities]"));
    }

    #[test]
    fn strips_markdown_links_and_truncates() {
        let desc = "Create a guest user with access to a [subset of Workspace capabilities](https://support.google.com/a/answer/16558545). This feature is currently in Alpha. Please reach out to support if you are interested in enabling this feature.";
        let result = truncate_description(desc, 120, true);
        // After stripping the link, the sentence boundary should work.
        assert!(result.contains("subset of Workspace capabilities."));
        assert!(!result.contains("https://"));
    }

    #[test]
    fn multibyte_safe() {
        let desc = "Résumé création für Ñoño — a long description that should be safely truncated at word boundaries without panicking on multi-byte chars";
        let result = truncate_description(desc, 30, true);
        assert!(result.ends_with('…') || result.chars().count() <= 30);
    }

    #[test]
    fn empty_and_whitespace() {
        assert_eq!(truncate_description("", 100, true), "");
        assert_eq!(truncate_description("   ", 100, true), "");
        assert_eq!(truncate_description("", 0, true), "");
    }

    #[test]
    fn test_strip_markdown_links() {
        assert_eq!(strip_markdown_links("[text](http://example.com)"), "text");
        assert_eq!(
            strip_markdown_links("Use [this link](http://a.com) and [that](http://b.com) too"),
            "Use this link and that too"
        );
        assert_eq!(strip_markdown_links("no links here"), "no links here");
        // Incomplete link syntax should be left alone
        assert_eq!(strip_markdown_links("[broken"), "[broken");
        assert_eq!(strip_markdown_links("[text]no-parens"), "[text]no-parens");
    }

    #[test]
    fn preserves_sentence_ending_at_limit() {
        let desc = "Deletes a user.";
        assert_eq!(truncate_description(desc, 15, true), "Deletes a user.");
    }

    #[test]
    fn does_not_cut_url_looking_periods() {
        // Periods in URLs or abbreviations like "v1." shouldn't be treated as sentence ends
        // unless followed by a space
        let desc = "See the docs at developers.google.com for more details on this API endpoint";
        let result = truncate_description(desc, 50, true);
        // Should truncate at word boundary, not at "developers."
        assert!(result.ends_with('…'));
    }

    #[test]
    fn sentence_boundary_at_exact_limit() {
        // Period falls exactly at the end of the prefix — should still detect it
        let desc = "This is a complete sentence. And more text follows here.";
        let result = truncate_description(desc, 28, true);
        assert_eq!(result, "This is a complete sentence.");
    }

    #[test]
    fn zero_max_chars() {
        assert_eq!(truncate_description("anything", 0, true), "");
    }
}

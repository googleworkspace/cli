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

//! Markdown-to-Google-Docs batchUpdate conversion.
//!
//! Parses markdown using pulldown-cmark and produces a list of Google Docs API
//! request objects (insertText, updateParagraphStyle, updateTextStyle,
//! createParagraphBullets) suitable for a single `documents.batchUpdate` call.
//!
//! Strategy:
//! 1. Walk the markdown AST, collecting text and formatting metadata.
//! 2. Concatenate all text into one string while tracking 1-based character
//!    indices for every formatted range.
//! 3. Emit one `insertText` request (endOfSegmentLocation).
//! 4. Emit N formatting requests using the pre-computed ranges.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert a markdown string into a vector of Google Docs batchUpdate request
/// objects. The first element is always an `insertText`; the rest are styling
/// requests.
pub fn markdown_to_batch_requests(markdown: &str) -> Vec<Value> {
    let (full_text, format_ranges) = collect_text_and_ranges(markdown);

    if full_text.is_empty() {
        return vec![];
    }

    let mut requests: Vec<Value> = Vec::new();

    // 1. Single insertText with the full concatenated text.
    requests.push(json!({
        "insertText": {
            "text": full_text,
            "endOfSegmentLocation": {
                "segmentId": ""
            }
        }
    }));

    // 2. Formatting requests. Because all text is inserted in one shot, the
    //    1-based indices we tracked are stable.
    for range in &format_ranges {
        match &range.kind {
            FormatKind::Heading(level) => {
                let named_style = heading_level_to_named_style(*level);
                requests.push(json!({
                    "updateParagraphStyle": {
                        "paragraphStyle": {
                            "namedStyleType": named_style
                        },
                        "range": {
                            "startIndex": range.start,
                            "endIndex": range.end
                        },
                        "fields": "namedStyleType"
                    }
                }));
            }
            FormatKind::Bold => {
                requests.push(json!({
                    "updateTextStyle": {
                        "textStyle": {
                            "bold": true
                        },
                        "range": {
                            "startIndex": range.start,
                            "endIndex": range.end
                        },
                        "fields": "bold"
                    }
                }));
            }
            FormatKind::Italic => {
                requests.push(json!({
                    "updateTextStyle": {
                        "textStyle": {
                            "italic": true
                        },
                        "range": {
                            "startIndex": range.start,
                            "endIndex": range.end
                        },
                        "fields": "italic"
                    }
                }));
            }
            FormatKind::InlineCode => {
                requests.push(json!({
                    "updateTextStyle": {
                        "textStyle": {
                            "weightedFontFamily": {
                                "fontFamily": "Courier New"
                            }
                        },
                        "range": {
                            "startIndex": range.start,
                            "endIndex": range.end
                        },
                        "fields": "weightedFontFamily"
                    }
                }));
            }
            FormatKind::CodeBlock => {
                requests.push(json!({
                    "updateTextStyle": {
                        "textStyle": {
                            "weightedFontFamily": {
                                "fontFamily": "Courier New"
                            }
                        },
                        "range": {
                            "startIndex": range.start,
                            "endIndex": range.end
                        },
                        "fields": "weightedFontFamily"
                    }
                }));
            }
            FormatKind::Link(url) => {
                requests.push(json!({
                    "updateTextStyle": {
                        "textStyle": {
                            "link": {
                                "url": url
                            }
                        },
                        "range": {
                            "startIndex": range.start,
                            "endIndex": range.end
                        },
                        "fields": "link"
                    }
                }));
            }
            FormatKind::UnorderedList => {
                requests.push(json!({
                    "createParagraphBullets": {
                        "range": {
                            "startIndex": range.start,
                            "endIndex": range.end
                        },
                        "bulletPreset": "BULLET_DISC_CIRCLE_SQUARE"
                    }
                }));
            }
            FormatKind::OrderedList => {
                requests.push(json!({
                    "createParagraphBullets": {
                        "range": {
                            "startIndex": range.start,
                            "endIndex": range.end
                        },
                        "bulletPreset": "NUMBERED_DECIMAL_ALPHA_ROMAN"
                    }
                }));
            }
        }
    }

    requests
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum FormatKind {
    Heading(u8),
    Bold,
    Italic,
    InlineCode,
    CodeBlock,
    Link(String),
    UnorderedList,
    OrderedList,
}

#[derive(Debug, Clone)]
struct FormatRange {
    start: usize, // 1-based
    end: usize,   // 1-based, exclusive
    kind: FormatKind,
}

// ---------------------------------------------------------------------------
// Markdown walking
// ---------------------------------------------------------------------------

/// Walk the markdown AST. Returns (full_text, format_ranges).
///
/// `full_text` is the concatenated plain text (with newlines for paragraph
/// boundaries). `format_ranges` uses 1-based indices compatible with the
/// Google Docs API.
fn collect_text_and_ranges(markdown: &str) -> (String, Vec<FormatRange>) {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, opts);

    let mut full_text = String::new();
    // Cursor tracks the *next* 1-based index we will write to.
    // Google Docs body starts at index 1.
    let mut cursor: usize = 1;

    let mut format_ranges: Vec<FormatRange> = Vec::new();

    // Stack of (tag, start_index) for nested inline styles.
    let mut style_stack: Vec<(StyleEntry, usize)> = Vec::new();

    // Track list context
    let mut list_stack: Vec<ListContext> = Vec::new();

    // Track whether we are inside a code block
    // code block state tracked via code_block_start
    let mut code_block_start: usize = 0;

    // Track heading context
    // heading state tracked via heading_start/heading_level
    let mut heading_level: u8 = 0;
    let mut heading_start: usize = 0;

    // Need to add newlines between paragraphs but not double at start
    let mut needs_paragraph_break = false;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    if needs_paragraph_break {
                        full_text.push('\n');
                        cursor += 1;
                    }
                    // heading started (tracked via heading_start)
                    heading_level = heading_level_to_u8(level);
                    heading_start = cursor;
                }
                Tag::Paragraph => {
                    if needs_paragraph_break {
                        full_text.push('\n');
                        cursor += 1;
                    }
                }
                Tag::Emphasis => {
                    style_stack.push((StyleEntry::Italic, cursor));
                }
                Tag::Strong => {
                    style_stack.push((StyleEntry::Bold, cursor));
                }
                Tag::Link { dest_url, .. } => {
                    style_stack.push((StyleEntry::Link(dest_url.to_string()), cursor));
                }
                Tag::CodeBlock(_) => {
                    if needs_paragraph_break {
                        full_text.push('\n');
                        cursor += 1;
                    }
                    // code block started (tracked via code_block_start)
                    code_block_start = cursor;
                }
                Tag::List(first_item) => {
                    if needs_paragraph_break && list_stack.is_empty() {
                        full_text.push('\n');
                        cursor += 1;
                    }
                    let ordered = first_item.is_some();
                    list_stack.push(ListContext {
                        ordered,
                        item_start: 0,
                    });
                }
                Tag::Item => {
                    // Mark the start for bullet formatting
                    if let Some(ctx) = list_stack.last_mut() {
                        // Add newline separator between list items (but not before the first)
                        if ctx.item_start > 0 {
                            full_text.push('\n');
                            cursor += 1;
                        }
                        ctx.item_start = cursor;
                    }
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    // Append newline after heading text
                    full_text.push('\n');
                    let heading_end = cursor; // end before the newline char for style range
                    cursor += 1;
                    format_ranges.push(FormatRange {
                        start: heading_start,
                        end: heading_end,
                        kind: FormatKind::Heading(heading_level),
                    });
                    // heading ended
                    needs_paragraph_break = true;
                }
                TagEnd::Paragraph => {
                    full_text.push('\n');
                    cursor += 1;
                    needs_paragraph_break = true;
                }
                TagEnd::Emphasis => {
                    if let Some((StyleEntry::Italic, start)) = style_stack.pop() {
                        format_ranges.push(FormatRange {
                            start,
                            end: cursor,
                            kind: FormatKind::Italic,
                        });
                    }
                }
                TagEnd::Strong => {
                    if let Some((StyleEntry::Bold, start)) = style_stack.pop() {
                        format_ranges.push(FormatRange {
                            start,
                            end: cursor,
                            kind: FormatKind::Bold,
                        });
                    }
                }
                TagEnd::Link => {
                    if let Some((StyleEntry::Link(url), start)) = style_stack.pop() {
                        format_ranges.push(FormatRange {
                            start,
                            end: cursor,
                            kind: FormatKind::Link(url),
                        });
                    }
                }
                TagEnd::CodeBlock => {
                    // The code block text already got appended via Event::Text.
                    // Don't add trailing newline if text already ends with one.
                    if !full_text.ends_with('\n') {
                        full_text.push('\n');
                        cursor += 1;
                    }
                    format_ranges.push(FormatRange {
                        start: code_block_start,
                        end: cursor,
                        kind: FormatKind::CodeBlock,
                    });
                    // code block ended
                    needs_paragraph_break = true;
                }
                TagEnd::Item => {
                    // Record bullet range for this item
                    if let Some(ctx) = list_stack.last() {
                        let kind = if ctx.ordered {
                            FormatKind::OrderedList
                        } else {
                            FormatKind::UnorderedList
                        };
                        format_ranges.push(FormatRange {
                            start: ctx.item_start,
                            end: cursor,
                            kind,
                        });
                    }
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                    // Add trailing newline after list
                    full_text.push('\n');
                    cursor += 1;
                    needs_paragraph_break = true;
                }
                _ => {}
            },
            Event::Text(text) => {
                let s = text.as_ref();
                full_text.push_str(s);
                cursor += s.encode_utf16().count();
            }
            Event::Code(code) => {
                // Inline code
                let s = code.as_ref();
                let start = cursor;
                full_text.push_str(s);
                cursor += s.encode_utf16().count();
                format_ranges.push(FormatRange {
                    start,
                    end: cursor,
                    kind: FormatKind::InlineCode,
                });
            }
            Event::SoftBreak => {
                full_text.push(' ');
                cursor += 1;
            }
            Event::HardBreak => {
                full_text.push('\n');
                cursor += 1;
            }
            _ => {}
        }
    }

    (full_text, format_ranges)
}

#[derive(Debug)]
enum StyleEntry {
    Bold,
    Italic,
    Link(String),
}

#[derive(Debug)]
struct ListContext {
    ordered: bool,
    /// 1-based start index of current item (0 means no item started yet).
    item_start: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn heading_level_to_named_style(level: u8) -> &'static str {
    match level {
        1 => "HEADING_1",
        2 => "HEADING_2",
        3 => "HEADING_3",
        4 => "HEADING_4",
        5 => "HEADING_5",
        6 => "HEADING_6",
        _ => "NORMAL_TEXT",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: extract the full inserted text from the first request.
    fn inserted_text(requests: &[Value]) -> &str {
        requests[0]["insertText"]["text"].as_str().unwrap()
    }

    // Helper: count requests of a given type.
    fn count_request_type(requests: &[Value], key: &str) -> usize {
        requests.iter().filter(|r| r.get(key).is_some()).count()
    }

    #[test]
    fn test_plain_text_passthrough() {
        let requests = markdown_to_batch_requests("Hello, world!");
        assert_eq!(requests.len(), 1, "plain text should produce only insertText");
        assert_eq!(inserted_text(&requests), "Hello, world!\n");
    }

    #[test]
    fn test_empty_input() {
        let requests = markdown_to_batch_requests("");
        assert!(requests.is_empty());
    }

    #[test]
    fn test_heading_conversion() {
        let md = "# Title\n\n## Subtitle\n\n### Section\n";
        let requests = markdown_to_batch_requests(md);

        // 1 insertText + 3 headings
        assert_eq!(count_request_type(&requests, "insertText"), 1);
        assert_eq!(count_request_type(&requests, "updateParagraphStyle"), 3);

        // Check heading levels
        let h1 = &requests[1]["updateParagraphStyle"];
        assert_eq!(
            h1["paragraphStyle"]["namedStyleType"].as_str().unwrap(),
            "HEADING_1"
        );
        let h2 = &requests[2]["updateParagraphStyle"];
        assert_eq!(
            h2["paragraphStyle"]["namedStyleType"].as_str().unwrap(),
            "HEADING_2"
        );
        let h3 = &requests[3]["updateParagraphStyle"];
        assert_eq!(
            h3["paragraphStyle"]["namedStyleType"].as_str().unwrap(),
            "HEADING_3"
        );
    }

    #[test]
    fn test_bold_italic_conversion() {
        let md = "This is **bold** and *italic* text.";
        let requests = markdown_to_batch_requests(md);

        // 1 insertText + 1 bold + 1 italic
        assert_eq!(count_request_type(&requests, "insertText"), 1);
        assert_eq!(count_request_type(&requests, "updateTextStyle"), 2);

        let text = inserted_text(&requests);
        assert!(text.contains("bold"));
        assert!(text.contains("italic"));
        // No markdown syntax in output
        assert!(!text.contains("**"));
        assert!(!text.contains("*italic*"));

        // Bold range
        let bold_req = requests
            .iter()
            .find(|r| {
                r.get("updateTextStyle")
                    .and_then(|s| s["textStyle"].get("bold"))
                    .is_some()
            })
            .unwrap();
        assert_eq!(
            bold_req["updateTextStyle"]["textStyle"]["bold"]
                .as_bool()
                .unwrap(),
            true
        );

        // Italic range
        let italic_req = requests
            .iter()
            .find(|r| {
                r.get("updateTextStyle")
                    .and_then(|s| s["textStyle"].get("italic"))
                    .is_some()
            })
            .unwrap();
        assert_eq!(
            italic_req["updateTextStyle"]["textStyle"]["italic"]
                .as_bool()
                .unwrap(),
            true
        );
    }

    #[test]
    fn test_inline_code() {
        let md = "Use `println!` to print.";
        let requests = markdown_to_batch_requests(md);

        let text = inserted_text(&requests);
        assert!(text.contains("println!"));
        assert!(!text.contains('`'));

        let code_req = requests
            .iter()
            .find(|r| {
                r.get("updateTextStyle")
                    .and_then(|s| s["textStyle"].get("weightedFontFamily"))
                    .is_some()
            })
            .unwrap();
        assert_eq!(
            code_req["updateTextStyle"]["textStyle"]["weightedFontFamily"]["fontFamily"]
                .as_str()
                .unwrap(),
            "Courier New"
        );
    }

    #[test]
    fn test_index_tracking_correctness() {
        // "Hello **world**\n" -> text = "Hello world\n"
        // "Hello " = indices 1..7 (6 chars), "world" = 7..12 (5 chars), "\n" = 12..13
        let md = "Hello **world**";
        let requests = markdown_to_batch_requests(md);

        let text = inserted_text(&requests);
        assert_eq!(text, "Hello world\n");

        let bold_req = requests
            .iter()
            .find(|r| r.get("updateTextStyle").is_some())
            .unwrap();
        let range = &bold_req["updateTextStyle"]["range"];
        // "world" starts at 1-based index 7, ends at 12
        assert_eq!(range["startIndex"].as_u64().unwrap(), 7);
        assert_eq!(range["endIndex"].as_u64().unwrap(), 12);
    }

    #[test]
    fn test_mixed_formatting() {
        let md = "# My Doc\n\nSome **bold** and *italic* text.\n\n- item one\n- item two\n";
        let requests = markdown_to_batch_requests(md);

        let text = inserted_text(&requests);
        // Should contain all plain text
        assert!(text.contains("My Doc"));
        assert!(text.contains("bold"));
        assert!(text.contains("italic"));
        assert!(text.contains("item one"));
        assert!(text.contains("item two"));

        // Should have heading, bold, italic, and bullets
        assert!(count_request_type(&requests, "updateParagraphStyle") >= 1);
        assert!(count_request_type(&requests, "updateTextStyle") >= 2);
        assert!(count_request_type(&requests, "createParagraphBullets") >= 2);
    }

    #[test]
    fn test_unordered_list() {
        let md = "- alpha\n- beta\n";
        let requests = markdown_to_batch_requests(md);

        let bullet_count = count_request_type(&requests, "createParagraphBullets");
        assert_eq!(bullet_count, 2);

        let bullet_req = requests
            .iter()
            .find(|r| r.get("createParagraphBullets").is_some())
            .unwrap();
        assert_eq!(
            bullet_req["createParagraphBullets"]["bulletPreset"]
                .as_str()
                .unwrap(),
            "BULLET_DISC_CIRCLE_SQUARE"
        );
    }

    #[test]
    fn test_ordered_list() {
        let md = "1. first\n2. second\n";
        let requests = markdown_to_batch_requests(md);

        let bullet_count = count_request_type(&requests, "createParagraphBullets");
        assert_eq!(bullet_count, 2);

        let bullet_req = requests
            .iter()
            .find(|r| r.get("createParagraphBullets").is_some())
            .unwrap();
        assert_eq!(
            bullet_req["createParagraphBullets"]["bulletPreset"]
                .as_str()
                .unwrap(),
            "NUMBERED_DECIMAL_ALPHA_ROMAN"
        );
    }

    #[test]
    fn test_code_block() {
        let md = "```\nfn main() {}\n```\n";
        let requests = markdown_to_batch_requests(md);

        let text = inserted_text(&requests);
        assert!(text.contains("fn main() {}"));

        // Code block gets monospace styling
        let code_req = requests
            .iter()
            .find(|r| {
                r.get("updateTextStyle")
                    .and_then(|s| s["textStyle"].get("weightedFontFamily"))
                    .is_some()
            })
            .unwrap();
        assert_eq!(
            code_req["updateTextStyle"]["textStyle"]["weightedFontFamily"]["fontFamily"]
                .as_str()
                .unwrap(),
            "Courier New"
        );
    }

    #[test]
    fn test_link() {
        let md = "Visit [Google](https://google.com) today.";
        let requests = markdown_to_batch_requests(md);

        let text = inserted_text(&requests);
        assert!(text.contains("Google"));
        assert!(!text.contains("https://google.com"));

        let link_req = requests
            .iter()
            .find(|r| {
                r.get("updateTextStyle")
                    .and_then(|s| s["textStyle"].get("link"))
                    .is_some()
            })
            .unwrap();
        assert_eq!(
            link_req["updateTextStyle"]["textStyle"]["link"]["url"]
                .as_str()
                .unwrap(),
            "https://google.com"
        );
    }

    #[test]
    fn test_heading_indices_sequential() {
        let md = "# A\n\n## B\n";
        let requests = markdown_to_batch_requests(md);

        let text = inserted_text(&requests);
        // "A\n" + "\n" (paragraph break) + "B\n" = "A\n\nB\n" (len 5)
        assert_eq!(text, "A\n\nB\n");

        let h1 = &requests[1]["updateParagraphStyle"]["range"];
        // "A" is at index 1..2
        assert_eq!(h1["startIndex"].as_u64().unwrap(), 1);
        assert_eq!(h1["endIndex"].as_u64().unwrap(), 2);

        let h2 = &requests[2]["updateParagraphStyle"]["range"];
        // "B" is at index 4..5 (after "A\n\n")
        assert_eq!(h2["startIndex"].as_u64().unwrap(), 4);
        assert_eq!(h2["endIndex"].as_u64().unwrap(), 5);
    }

    #[test]
    fn test_all_requests_have_valid_ranges() {
        let md = "# Title\n\nHello **bold** *italic* `code`\n\n- item\n";
        let requests = markdown_to_batch_requests(md);
        let text = inserted_text(&requests);
        let text_len = text.len();

        for req in &requests[1..] {
            // Find the range in whichever request type
            let range = if let Some(v) = req.get("updateParagraphStyle") {
                v.get("range")
            } else if let Some(v) = req.get("updateTextStyle") {
                v.get("range")
            } else if let Some(v) = req.get("createParagraphBullets") {
                v.get("range")
            } else {
                None
            };

            if let Some(range) = range {
                let start = range["startIndex"].as_u64().unwrap() as usize;
                let end = range["endIndex"].as_u64().unwrap() as usize;
                assert!(start >= 1, "start index must be >= 1, got {}", start);
                assert!(
                    end <= text_len + 1,
                    "end index {} exceeds text length + 1 ({})",
                    end,
                    text_len + 1
                );
                assert!(start < end, "start {} must be < end {}", start, end);
            }
        }
    }
}

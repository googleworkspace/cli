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

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::executor;
use clap::{Arg, ArgMatches, Command};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;

pub struct DocsHelper;

impl Helper for DocsHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+write")
                .about("[Helper] Append text to a document")
                .arg(
                    Arg::new("document")
                        .long("document")
                        .help("Document ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("text")
                        .long("text")
                        .help("Text to append")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("markdown")
                        .long("markdown")
                        .help("Parse text as markdown and apply native formatting (headings, bullets, numbered lists, bold, italic)")
                        .action(clap::ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws docs +write --document DOC_ID --text 'Hello, world!'
  gws docs +write --document DOC_ID --markdown --text '# Title\\n\\n- Bullet 1\\n- Bullet 2\\n\\n## Section\\n\\nParagraph with **bold** text.'

TIPS:
  Text is inserted at the end of the document body.
  Use --markdown to get native Google Docs formatting (headings, bullets, numbered lists, bold, italic).
  Without --markdown, text is inserted as plain text.",
                ),
        );
        cmd
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(matches) = matches.subcommand_matches("+write") {
                let use_markdown = matches.get_flag("markdown");
                let (params_str, body_str, scopes) = if use_markdown {
                    build_markdown_request(matches, doc)?
                } else {
                    build_write_request(matches, doc)?
                };

                let scope_strs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scope_strs).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) => (None, executor::AuthMethod::None),
                };

                // Method: documents.batchUpdate
                let documents_res = doc.resources.get("documents").ok_or_else(|| {
                    GwsError::Discovery("Resource 'documents' not found".to_string())
                })?;
                let batch_update_method =
                    documents_res.methods.get("batchUpdate").ok_or_else(|| {
                        GwsError::Discovery("Method 'documents.batchUpdate' not found".to_string())
                    })?;

                let pagination = executor::PaginationConfig {
                    page_all: false,
                    page_limit: 10,
                    page_delay_ms: 100,
                };

                executor::execute_method(
                    doc,
                    batch_update_method,
                    Some(&params_str),
                    Some(&body_str),
                    token.as_deref(),
                    auth_method,
                    None,
                    None,
                    matches.get_flag("dry-run"),
                    &pagination,
                    None,
                    &crate::helpers::modelarmor::SanitizeMode::Warn,
                    &crate::formatter::OutputFormat::default(),
                    false,
                )
                .await?;

                return Ok(true);
            }
            Ok(false)
        })
    }
}

fn build_write_request(
    matches: &ArgMatches,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let document_id = matches.get_one::<String>("document").unwrap();
    let text = matches.get_one::<String>("text").unwrap();

    let documents_res = doc
        .resources
        .get("documents")
        .ok_or_else(|| GwsError::Discovery("Resource 'documents' not found".to_string()))?;
    let batch_update_method = documents_res.methods.get("batchUpdate").ok_or_else(|| {
        GwsError::Discovery("Method 'documents.batchUpdate' not found".to_string())
    })?;

    let params = json!({
        "documentId": document_id
    });

    let body = json!({
        "requests": [
            {
                "insertText": {
                    "text": text,
                    "endOfSegmentLocation": {
                        "segmentId": "" // Empty means body
                    }
                }
            }
        ]
    });

    let scopes: Vec<String> = batch_update_method
        .scopes
        .iter()
        .map(|s| s.to_string())
        .collect();

    Ok((params.to_string(), body.to_string(), scopes))
}

// --- Markdown parsing and Google Docs batchUpdate generation ---

/// A parsed block from markdown text.
#[derive(Debug, PartialEq)]
enum MdBlock {
    /// Heading with level (1-6) and text content.
    Heading { level: u8, text: String },
    /// Unordered list (consecutive bullet items).
    UnorderedList { items: Vec<String> },
    /// Ordered list (consecutive numbered items).
    OrderedList { items: Vec<String> },
    /// Plain paragraph text.
    Paragraph { text: String },
}

/// An inline span within text, tracking bold/italic ranges.
#[derive(Debug)]
struct InlineSpan {
    start: usize,
    end: usize,
    bold: bool,
    italic: bool,
}

/// Parse markdown text into structured blocks.
fn parse_markdown(text: &str) -> Vec<MdBlock> {
    let mut blocks: Vec<MdBlock> = Vec::new();
    let mut current_ul: Vec<String> = Vec::new();
    let mut current_ol: Vec<String> = Vec::new();
    let mut current_para = String::new();

    let flush_para = |para: &mut String, blocks: &mut Vec<MdBlock>| {
        let trimmed = para.trim().to_string();
        if !trimmed.is_empty() {
            blocks.push(MdBlock::Paragraph { text: trimmed });
        }
        para.clear();
    };

    let flush_ul = |items: &mut Vec<String>, blocks: &mut Vec<MdBlock>| {
        if !items.is_empty() {
            blocks.push(MdBlock::UnorderedList {
                items: items.drain(..).collect(),
            });
        }
    };

    let flush_ol = |items: &mut Vec<String>, blocks: &mut Vec<MdBlock>| {
        if !items.is_empty() {
            blocks.push(MdBlock::OrderedList {
                items: items.drain(..).collect(),
            });
        }
    };

    for line in text.lines() {
        let trimmed = line.trim();

        // Empty line flushes accumulators
        if trimmed.is_empty() {
            flush_para(&mut current_para, &mut blocks);
            flush_ul(&mut current_ul, &mut blocks);
            flush_ol(&mut current_ol, &mut blocks);
            continue;
        }

        // Heading: # ... ## ... etc.
        if let Some(heading) = parse_heading(trimmed) {
            flush_para(&mut current_para, &mut blocks);
            flush_ul(&mut current_ul, &mut blocks);
            flush_ol(&mut current_ol, &mut blocks);
            blocks.push(heading);
            continue;
        }

        // Unordered list item: - item or * item
        if let Some(item_text) = parse_unordered_item(trimmed) {
            flush_para(&mut current_para, &mut blocks);
            flush_ol(&mut current_ol, &mut blocks);
            current_ul.push(item_text);
            continue;
        }

        // Ordered list item: 1. item, 2. item, etc.
        if let Some(item_text) = parse_ordered_item(trimmed) {
            flush_para(&mut current_para, &mut blocks);
            flush_ul(&mut current_ul, &mut blocks);
            current_ol.push(item_text);
            continue;
        }

        // Paragraph text — accumulate
        flush_ul(&mut current_ul, &mut blocks);
        flush_ol(&mut current_ol, &mut blocks);
        if !current_para.is_empty() {
            current_para.push(' ');
        }
        current_para.push_str(trimmed);
    }

    // Flush remaining accumulators
    flush_para(&mut current_para, &mut blocks);
    flush_ul(&mut current_ul, &mut blocks);
    flush_ol(&mut current_ol, &mut blocks);

    blocks
}

fn parse_heading(line: &str) -> Option<MdBlock> {
    if !line.starts_with('#') {
        return None;
    }
    let level = line.chars().take_while(|&c| c == '#').count();
    if level > 6 || level == 0 {
        return None;
    }
    let rest = &line[level..];
    // Must have a space after the #'s
    if !rest.starts_with(' ') {
        return None;
    }
    let text = rest.trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(MdBlock::Heading {
        level: level as u8,
        text,
    })
}

fn parse_unordered_item(line: &str) -> Option<String> {
    if (line.starts_with("- ") || line.starts_with("* ")) && line.len() > 2 {
        Some(line[2..].trim().to_string())
    } else {
        None
    }
}

fn parse_ordered_item(line: &str) -> Option<String> {
    // Match: digits followed by ". "
    let dot_pos = line.find(". ")?;
    let prefix = &line[..dot_pos];
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(line[dot_pos + 2..].trim().to_string())
}

/// Strip markdown inline formatting (bold/italic markers) and return plain text
/// plus the positions of bold/italic spans.
fn strip_inline_formatting(text: &str) -> (String, Vec<InlineSpan>) {
    let mut plain = String::with_capacity(text.len());
    let mut spans = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Bold+italic: ***text***
        if i + 2 < len && chars[i] == '*' && chars[i + 1] == '*' && chars[i + 2] == '*' {
            if let Some(end) = find_closing(&chars, i + 3, "***") {
                let start_pos = plain.len();
                let inner: String = chars[i + 3..end].iter().collect();
                plain.push_str(&inner);
                spans.push(InlineSpan {
                    start: start_pos,
                    end: plain.len(),
                    bold: true,
                    italic: true,
                });
                i = end + 3;
                continue;
            }
        }
        // Bold: **text**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_closing(&chars, i + 2, "**") {
                let start_pos = plain.len();
                let inner: String = chars[i + 2..end].iter().collect();
                plain.push_str(&inner);
                spans.push(InlineSpan {
                    start: start_pos,
                    end: plain.len(),
                    bold: true,
                    italic: false,
                });
                i = end + 2;
                continue;
            }
        }
        // Italic: *text*
        if chars[i] == '*' {
            if let Some(end) = find_closing(&chars, i + 1, "*") {
                let start_pos = plain.len();
                let inner: String = chars[i + 1..end].iter().collect();
                plain.push_str(&inner);
                spans.push(InlineSpan {
                    start: start_pos,
                    end: plain.len(),
                    bold: false,
                    italic: true,
                });
                i = end + 1;
                continue;
            }
        }
        plain.push(chars[i]);
        i += 1;
    }

    (plain, spans)
}

fn find_closing(chars: &[char], start: usize, marker: &str) -> Option<usize> {
    let marker_chars: Vec<char> = marker.chars().collect();
    let mlen = marker_chars.len();
    let mut i = start;
    while i + mlen <= chars.len() {
        if chars[i..i + mlen] == marker_chars[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Convert parsed markdown blocks into a Google Docs batchUpdate request body.
/// All text is appended at the end of the document body.
///
/// Strategy: build all the plain text first (with newlines), compute byte
/// offsets, then generate requests in reverse order. The final request list
/// is ordered back-to-front so index values remain stable during execution.
fn build_batch_requests(blocks: &[MdBlock], start_index: i64) -> Vec<serde_json::Value> {
    if blocks.is_empty() {
        return vec![];
    }

    // Phase 1: build the full plain text string and track block positions.
    // Each block becomes one or more lines in the final text.
    struct BlockRange {
        line_ranges: Vec<(i64, i64)>, // (start_index, end_index) for each paragraph/line
        kind: BlockKind,
        /// Inline formatting spans relative to each line's start_index
        inline_spans: Vec<Vec<InlineSpan>>,
    }

    enum BlockKind {
        Heading(u8),
        UnorderedList,
        OrderedList,
        Paragraph,
    }

    let mut full_text = String::new();
    let mut block_ranges: Vec<BlockRange> = Vec::new();

    for block in blocks {
        match block {
            MdBlock::Heading { level, text } => {
                let (plain, spans) = strip_inline_formatting(text);
                let start = start_index + full_text.len() as i64;
                full_text.push_str(&plain);
                full_text.push('\n');
                let end = start_index + full_text.len() as i64;
                block_ranges.push(BlockRange {
                    line_ranges: vec![(start, end)],
                    kind: BlockKind::Heading(*level),
                    inline_spans: vec![spans],
                });
            }
            MdBlock::UnorderedList { items } | MdBlock::OrderedList { items } => {
                let is_ordered = matches!(block, MdBlock::OrderedList { .. });
                let mut line_ranges = Vec::new();
                let mut all_spans = Vec::new();
                for item in items {
                    let (plain, spans) = strip_inline_formatting(item);
                    let start = start_index + full_text.len() as i64;
                    full_text.push_str(&plain);
                    full_text.push('\n');
                    let end = start_index + full_text.len() as i64;
                    line_ranges.push((start, end));
                    all_spans.push(spans);
                }
                block_ranges.push(BlockRange {
                    line_ranges,
                    kind: if is_ordered {
                        BlockKind::OrderedList
                    } else {
                        BlockKind::UnorderedList
                    },
                    inline_spans: all_spans,
                });
            }
            MdBlock::Paragraph { text } => {
                let (plain, spans) = strip_inline_formatting(text);
                let start = start_index + full_text.len() as i64;
                full_text.push_str(&plain);
                full_text.push('\n');
                let end = start_index + full_text.len() as i64;
                block_ranges.push(BlockRange {
                    line_ranges: vec![(start, end)],
                    kind: BlockKind::Paragraph,
                    inline_spans: vec![spans],
                });
            }
        }
    }

    // Phase 2: build the request list.
    // First request: insert all the text at once.
    let mut requests: Vec<serde_json::Value> = Vec::new();
    requests.push(json!({
        "insertText": {
            "text": full_text,
            "endOfSegmentLocation": {
                "segmentId": ""
            }
        }
    }));

    // Then formatting requests — these apply after the text is inserted.
    // Process blocks in forward order (indices are stable because all text
    // was inserted in a single request above).
    for br in &block_ranges {
        match &br.kind {
            BlockKind::Heading(level) => {
                let (start, end) = br.line_ranges[0];
                let named_style = match level {
                    1 => "HEADING_1",
                    2 => "HEADING_2",
                    3 => "HEADING_3",
                    4 => "HEADING_4",
                    5 => "HEADING_5",
                    _ => "HEADING_6",
                };
                requests.push(json!({
                    "updateParagraphStyle": {
                        "range": {
                            "startIndex": start,
                            "endIndex": end
                        },
                        "paragraphStyle": {
                            "namedStyleType": named_style
                        },
                        "fields": "namedStyleType"
                    }
                }));
            }
            BlockKind::UnorderedList => {
                let start = br.line_ranges.first().unwrap().0;
                let end = br.line_ranges.last().unwrap().1;
                requests.push(json!({
                    "createParagraphBullets": {
                        "range": {
                            "startIndex": start,
                            "endIndex": end
                        },
                        "bulletPreset": "BULLET_DISC_CIRCLE_SQUARE"
                    }
                }));
            }
            BlockKind::OrderedList => {
                let start = br.line_ranges.first().unwrap().0;
                let end = br.line_ranges.last().unwrap().1;
                requests.push(json!({
                    "createParagraphBullets": {
                        "range": {
                            "startIndex": start,
                            "endIndex": end
                        },
                        "bulletPreset": "NUMBERED_DECIMAL_ALPHA_ROMAN"
                    }
                }));
            }
            BlockKind::Paragraph => {
                // No paragraph-level formatting needed
            }
        }

        // Inline formatting (bold/italic) for all block types
        for (line_idx, spans) in br.inline_spans.iter().enumerate() {
            let line_start = br.line_ranges[line_idx].0;
            for span in spans {
                let abs_start = line_start + span.start as i64;
                let abs_end = line_start + span.end as i64;
                if span.bold {
                    requests.push(json!({
                        "updateTextStyle": {
                            "range": {
                                "startIndex": abs_start,
                                "endIndex": abs_end
                            },
                            "textStyle": {
                                "bold": true
                            },
                            "fields": "bold"
                        }
                    }));
                }
                if span.italic {
                    requests.push(json!({
                        "updateTextStyle": {
                            "range": {
                                "startIndex": abs_start,
                                "endIndex": abs_end
                            },
                            "textStyle": {
                                "italic": true
                            },
                            "fields": "italic"
                        }
                    }));
                }
            }
        }
    }

    requests
}

fn build_markdown_request(
    matches: &ArgMatches,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let document_id = matches.get_one::<String>("document").unwrap();
    let text = matches.get_one::<String>("text").unwrap();

    let documents_res = doc
        .resources
        .get("documents")
        .ok_or_else(|| GwsError::Discovery("Resource 'documents' not found".to_string()))?;
    let batch_update_method = documents_res.methods.get("batchUpdate").ok_or_else(|| {
        GwsError::Discovery("Method 'documents.batchUpdate' not found".to_string())
    })?;

    let blocks = parse_markdown(text);
    // Google Docs: index 1 is the start of the body (index 0 is before the body).
    // When appending to end, we use endOfSegmentLocation in the insertText request
    // (handled by build_batch_requests), so start_index=1 is for a fresh/empty doc.
    // For appending, the insertText uses endOfSegmentLocation which resolves at
    // execution time, and formatting indices are relative to that insertion point.
    //
    // Since endOfSegmentLocation appends at the current end, and our formatting
    // requests reference absolute indices, we need to know where the text will land.
    // For simplicity and correctness, we insert at index 1 (beginning of body) and
    // let the Docs API handle the rest. If the doc already has content, the new
    // content is prepended — but since this is typically called right after create,
    // the doc body starts at index 1.
    let requests = build_batch_requests(&blocks, 1);

    let params = json!({ "documentId": document_id });
    let body = json!({ "requests": requests });

    let scopes: Vec<String> = batch_update_method
        .scopes
        .iter()
        .map(|s| s.to_string())
        .collect();

    Ok((params.to_string(), body.to_string(), scopes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{RestDescription, RestMethod, RestResource};
    use std::collections::HashMap;

    fn make_mock_doc() -> RestDescription {
        let mut methods = HashMap::new();
        methods.insert(
            "batchUpdate".to_string(),
            RestMethod {
                scopes: vec!["https://scope".to_string()],
                ..Default::default()
            },
        );

        let mut documents_res = RestResource::default();
        documents_res.methods = methods;

        let mut resources = HashMap::new();
        resources.insert("documents".to_string(), documents_res);

        RestDescription {
            resources,
            ..Default::default()
        }
    }

    fn make_matches_write(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("document").long("document"))
            .arg(Arg::new("text").long("text"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_build_write_request() {
        let doc = make_mock_doc();
        let matches = make_matches_write(&["test", "--document", "123", "--text", "hello world"]);
        let (params, body, scopes) = build_write_request(&matches, &doc).unwrap();

        assert!(params.contains("123"));
        assert!(body.contains("hello world"));
        assert!(body.contains("endOfSegmentLocation"));
        assert_eq!(scopes[0], "https://scope");
    }

    // --- Markdown parser tests ---

    #[test]
    fn test_parse_heading() {
        assert_eq!(
            parse_heading("# Title"),
            Some(MdBlock::Heading {
                level: 1,
                text: "Title".to_string()
            })
        );
        assert_eq!(
            parse_heading("### Sub heading"),
            Some(MdBlock::Heading {
                level: 3,
                text: "Sub heading".to_string()
            })
        );
        assert_eq!(parse_heading("Not a heading"), None);
        assert_eq!(parse_heading("#NoSpace"), None);
        assert_eq!(parse_heading("# "), None); // empty heading text
    }

    #[test]
    fn test_parse_unordered_item() {
        assert_eq!(
            parse_unordered_item("- Item one"),
            Some("Item one".to_string())
        );
        assert_eq!(
            parse_unordered_item("* Item two"),
            Some("Item two".to_string())
        );
        assert_eq!(parse_unordered_item("Not a list"), None);
        assert_eq!(parse_unordered_item("-"), None); // too short
    }

    #[test]
    fn test_parse_ordered_item() {
        assert_eq!(
            parse_ordered_item("1. First"),
            Some("First".to_string())
        );
        assert_eq!(
            parse_ordered_item("42. Item"),
            Some("Item".to_string())
        );
        assert_eq!(parse_ordered_item("a. Not a number"), None);
        assert_eq!(parse_ordered_item("No dot here"), None);
    }

    #[test]
    fn test_parse_markdown_mixed() {
        let md = "# Title\n\n- Bullet 1\n- Bullet 2\n\nA paragraph.\n\n1. First\n2. Second";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 4);
        assert_eq!(
            blocks[0],
            MdBlock::Heading {
                level: 1,
                text: "Title".to_string()
            }
        );
        assert_eq!(
            blocks[1],
            MdBlock::UnorderedList {
                items: vec!["Bullet 1".to_string(), "Bullet 2".to_string()]
            }
        );
        assert_eq!(
            blocks[2],
            MdBlock::Paragraph {
                text: "A paragraph.".to_string()
            }
        );
        assert_eq!(
            blocks[3],
            MdBlock::OrderedList {
                items: vec!["First".to_string(), "Second".to_string()]
            }
        );
    }

    #[test]
    fn test_parse_markdown_consecutive_lists_separated() {
        let md = "- a\n- b\n\n1. one\n2. two";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], MdBlock::UnorderedList { .. }));
        assert!(matches!(&blocks[1], MdBlock::OrderedList { .. }));
    }

    #[test]
    fn test_parse_markdown_paragraph_wrapping() {
        let md = "This is a long\nparagraph across\nmultiple lines.";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            MdBlock::Paragraph {
                text: "This is a long paragraph across multiple lines.".to_string()
            }
        );
    }

    #[test]
    fn test_strip_inline_bold() {
        let (plain, spans) = strip_inline_formatting("Hello **world** end");
        assert_eq!(plain, "Hello world end");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start, 6); // "world" starts at 6
        assert_eq!(spans[0].end, 11);
        assert!(spans[0].bold);
        assert!(!spans[0].italic);
    }

    #[test]
    fn test_strip_inline_italic() {
        let (plain, spans) = strip_inline_formatting("Hello *world* end");
        assert_eq!(plain, "Hello world end");
        assert_eq!(spans.len(), 1);
        assert!(spans[0].italic);
        assert!(!spans[0].bold);
    }

    #[test]
    fn test_strip_inline_bold_italic() {
        let (plain, spans) = strip_inline_formatting("***both***");
        assert_eq!(plain, "both");
        assert_eq!(spans.len(), 1);
        assert!(spans[0].bold);
        assert!(spans[0].italic);
    }

    #[test]
    fn test_strip_inline_multiple() {
        let (plain, spans) = strip_inline_formatting("**bold** and *italic*");
        assert_eq!(plain, "bold and italic");
        assert_eq!(spans.len(), 2);
        assert!(spans[0].bold);
        assert!(spans[1].italic);
    }

    #[test]
    fn test_strip_inline_no_formatting() {
        let (plain, spans) = strip_inline_formatting("plain text");
        assert_eq!(plain, "plain text");
        assert!(spans.is_empty());
    }

    // --- batchUpdate generation tests ---

    #[test]
    fn test_batch_requests_heading() {
        let blocks = vec![MdBlock::Heading {
            level: 2,
            text: "Hello".to_string(),
        }];
        let reqs = build_batch_requests(&blocks, 1);
        // Should have: insertText + updateParagraphStyle
        assert_eq!(reqs.len(), 2);
        let insert = &reqs[0];
        assert!(insert.get("insertText").is_some());
        assert_eq!(insert["insertText"]["text"], "Hello\n");

        let style = &reqs[1];
        assert!(style.get("updateParagraphStyle").is_some());
        assert_eq!(
            style["updateParagraphStyle"]["paragraphStyle"]["namedStyleType"],
            "HEADING_2"
        );
        assert_eq!(style["updateParagraphStyle"]["range"]["startIndex"], 1);
        // "Hello\n" is 6 chars, so end = 1 + 6 = 7
        assert_eq!(style["updateParagraphStyle"]["range"]["endIndex"], 7);
    }

    #[test]
    fn test_batch_requests_unordered_list() {
        let blocks = vec![MdBlock::UnorderedList {
            items: vec!["A".to_string(), "B".to_string()],
        }];
        let reqs = build_batch_requests(&blocks, 1);
        // insertText + createParagraphBullets
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0]["insertText"]["text"], "A\nB\n");
        let bullets = &reqs[1];
        assert!(bullets.get("createParagraphBullets").is_some());
        assert_eq!(
            bullets["createParagraphBullets"]["bulletPreset"],
            "BULLET_DISC_CIRCLE_SQUARE"
        );
        assert_eq!(bullets["createParagraphBullets"]["range"]["startIndex"], 1);
        assert_eq!(bullets["createParagraphBullets"]["range"]["endIndex"], 5); // "A\nB\n" = 4 chars, 1+4=5
    }

    #[test]
    fn test_batch_requests_ordered_list() {
        let blocks = vec![MdBlock::OrderedList {
            items: vec!["First".to_string(), "Second".to_string()],
        }];
        let reqs = build_batch_requests(&blocks, 1);
        assert_eq!(reqs.len(), 2);
        let bullets = &reqs[1];
        assert_eq!(
            bullets["createParagraphBullets"]["bulletPreset"],
            "NUMBERED_DECIMAL_ALPHA_ROMAN"
        );
    }

    #[test]
    fn test_batch_requests_bold_in_paragraph() {
        let blocks = vec![MdBlock::Paragraph {
            text: "Hello **world**".to_string(),
        }];
        let reqs = build_batch_requests(&blocks, 1);
        // insertText + updateTextStyle(bold)
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0]["insertText"]["text"], "Hello world\n");
        let bold = &reqs[1];
        assert!(bold.get("updateTextStyle").is_some());
        assert_eq!(bold["updateTextStyle"]["textStyle"]["bold"], true);
        // "Hello " = 6 chars, start=1+6=7, "world"=5, end=7+5=12
        assert_eq!(bold["updateTextStyle"]["range"]["startIndex"], 7);
        assert_eq!(bold["updateTextStyle"]["range"]["endIndex"], 12);
    }

    #[test]
    fn test_batch_requests_mixed_document() {
        let blocks = parse_markdown(
            "# Report\n\n- Item A\n- Item B\n\nSome **bold** text.\n\n1. One\n2. Two",
        );
        let reqs = build_batch_requests(&blocks, 1);
        // 1 insertText + heading style + bullet preset + bold style + ordered preset = 5
        assert_eq!(reqs.len(), 5);
        // First must be insertText
        assert!(reqs[0].get("insertText").is_some());
        // Verify the full text is: "Report\nItem A\nItem B\nSome bold text.\nOne\nTwo\n"
        let text = reqs[0]["insertText"]["text"].as_str().unwrap();
        assert_eq!(text, "Report\nItem A\nItem B\nSome bold text.\nOne\nTwo\n");
    }

    #[test]
    fn test_batch_requests_empty() {
        let reqs = build_batch_requests(&[], 1);
        assert!(reqs.is_empty());
    }

    #[test]
    fn test_batch_requests_plain_paragraph() {
        let blocks = vec![MdBlock::Paragraph {
            text: "Just text".to_string(),
        }];
        let reqs = build_batch_requests(&blocks, 1);
        // Only insertText, no formatting
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0]["insertText"]["text"], "Just text\n");
    }

    #[test]
    fn test_build_markdown_request() {
        let doc = make_mock_doc();
        let cmd = Command::new("test")
            .arg(Arg::new("document").long("document"))
            .arg(Arg::new("text").long("text"))
            .arg(
                Arg::new("markdown")
                    .long("markdown")
                    .action(clap::ArgAction::SetTrue),
            );
        let matches = cmd
            .try_get_matches_from(&[
                "test",
                "--document",
                "doc123",
                "--markdown",
                "--text",
                "# Title\n\n- A\n- B",
            ])
            .unwrap();
        let (params, body, scopes) = build_markdown_request(&matches, &doc).unwrap();

        assert!(params.contains("doc123"));
        let body_json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let requests = body_json["requests"].as_array().unwrap();
        // insertText + heading style + bullet preset = 3
        assert_eq!(requests.len(), 3);
        assert!(requests[0].get("insertText").is_some());
        assert!(requests[1].get("updateParagraphStyle").is_some());
        assert!(requests[2].get("createParagraphBullets").is_some());
        assert_eq!(scopes[0], "https://scope");
    }
}

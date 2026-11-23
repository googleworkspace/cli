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

//! Google Docs JSON (`documents.get`) → Markdown and plain text.
//!
//! `docs +export` prefers Drive's own Markdown export. This converter is the
//! fallback for documents Drive will not export (the export is capped at
//! 10 MB, which image-heavy documents exceed), and the source of image links.

use serde_json::Value;

/// Fonts treated as monospace, i.e. as code.
const MONOSPACE_FONTS: &[&str] = &[
    "courier new",
    "courier",
    "roboto mono",
    "source code pro",
    "consolas",
    "inconsolata",
    "fira code",
    "fira mono",
    "jetbrains mono",
    "ibm plex mono",
    "space mono",
    "ubuntu mono",
    "menlo",
    "monaco",
    "cousine",
    "droid sans mono",
    "noto sans mono",
    "pt mono",
    "anonymous pro",
];

/// Stand-in for a horizontal rule while a paragraph is rendered.
const HR: &str = "\u{0}HR";

/// Glyph types that number a list.
const ORDERED_GLYPHS: &[&str] = &[
    "DECIMAL",
    "ZERO_DECIMAL",
    "UPPER_ALPHA",
    "ALPHA",
    "UPPER_ROMAN",
    "ROMAN",
];

fn body_content(doc: &Value) -> &[Value] {
    let body = doc.get("body").or_else(|| {
        doc.get("tabs")
            .and_then(Value::as_array)
            .and_then(|t| t.first())
            .and_then(|t| t.get("documentTab"))
            .and_then(|t| t.get("body"))
    });
    body.and_then(|b| b.get("content"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn inline_objects(doc: &Value) -> Option<&Value> {
    doc.get("inlineObjects").or_else(|| {
        doc.get("tabs")
            .and_then(Value::as_array)
            .and_then(|t| t.first())
            .and_then(|t| t.get("documentTab"))
            .and_then(|t| t.get("inlineObjects"))
    })
}

fn lists(doc: &Value) -> Option<&Value> {
    doc.get("lists").or_else(|| {
        doc.get("tabs")
            .and_then(Value::as_array)
            .and_then(|t| t.first())
            .and_then(|t| t.get("documentTab"))
            .and_then(|t| t.get("lists"))
    })
}

/// The URI of an inline image: its original source if Docs kept one,
/// otherwise the (short-lived) rendered copy.
fn image_uri(doc: &Value, object_id: &str) -> Option<(String, String)> {
    let embedded = inline_objects(doc)?
        .get(object_id)?
        .get("inlineObjectProperties")?
        .get("embeddedObject")?;
    let props = embedded.get("imageProperties")?;
    let uri = props
        .get("sourceUri")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| props.get("contentUri").and_then(Value::as_str))?;
    let alt = embedded
        .get("description")
        .or_else(|| embedded.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some((uri.to_string(), alt))
}

/// Every inline image in the body, tables included, in document order.
pub(crate) fn inline_image_uris(doc: &Value) -> Vec<String> {
    fn walk(doc: &Value, content: &[Value], out: &mut Vec<String>) {
        for el in content {
            if let Some(p) = el.get("paragraph") {
                for e in p
                    .get("elements")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    if let Some(id) = e
                        .get("inlineObjectElement")
                        .and_then(|i| i.get("inlineObjectId"))
                        .and_then(Value::as_str)
                    {
                        if let Some((uri, _)) = image_uri(doc, id) {
                            out.push(uri);
                        }
                    }
                }
            } else if let Some(t) = el.get("table") {
                for row in t
                    .get("tableRows")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    for cell in row
                        .get("tableCells")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if let Some(c) = cell.get("content").and_then(Value::as_array) {
                            walk(doc, c, out);
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(doc, body_content(doc), &mut out);
    out
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: Option<String>,
}

fn run_style(text_style: Option<&Value>) -> Style {
    let ts = match text_style {
        Some(t) => t,
        None => return Style::default(),
    };
    let flag = |k: &str| ts.get(k).and_then(Value::as_bool).unwrap_or(false);
    let font = ts
        .get("weightedFontFamily")
        .and_then(|f| f.get("fontFamily"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let link = ts.get("link").and_then(|l| {
        l.get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                l.get("headingId")
                    .and_then(Value::as_str)
                    .map(|h| format!("#heading={h}"))
            })
    });
    Style {
        bold: flag("bold"),
        italic: flag("italic"),
        strike: flag("strikethrough"),
        code: MONOSPACE_FONTS.contains(&font.as_str()),
        link,
    }
}

/// Escapes characters that Markdown would otherwise read as syntax.
fn escape_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escapes a paragraph's first characters if they would start a block.
fn escape_line_start(text: &str) -> String {
    let trimmed = text.trim_start();
    let lead = &text[..text.len() - trimmed.len()];
    let needs = trimmed.starts_with('#')
        || trimmed.starts_with('>')
        || trimmed.starts_with("- ")
        || trimmed.starts_with("+ ")
        || trimmed.starts_with("---")
        || trimmed.starts_with("===");
    if needs {
        return format!("{lead}\\{trimmed}");
    }
    // "1. text" would start an ordered list.
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && trimmed[digits..].starts_with(['.', ')']) {
        return format!("{lead}{}\\{}", &trimmed[..digits], &trimmed[digits..]);
    }
    text.to_string()
}

/// One styled piece of a paragraph, before Markdown markers are added.
struct Piece {
    text: String,
    style: Style,
    /// Already Markdown (an image), not to be escaped.
    raw: bool,
}

fn paragraph_pieces(doc: &Value, p: &Value) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for e in p
        .get("elements")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if let Some(tr) = e.get("textRun") {
            let content = tr.get("content").and_then(Value::as_str).unwrap_or("");
            if content.is_empty() {
                continue;
            }
            pieces.push(Piece {
                text: content.to_string(),
                style: run_style(tr.get("textStyle")),
                raw: false,
            });
        } else if let Some(io) = e.get("inlineObjectElement") {
            let id = io
                .get("inlineObjectId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if let Some((uri, alt)) = image_uri(doc, id) {
                pieces.push(Piece {
                    text: format!("![{}](<{}>)", escape_inline(&alt), uri),
                    style: Style::default(),
                    raw: true,
                });
            }
        } else if let Some(rl) = e.get("richLink") {
            let props = rl.get("richLinkProperties");
            let uri = props
                .and_then(|p| p.get("uri"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let title = props
                .and_then(|p| p.get("title"))
                .and_then(Value::as_str)
                .unwrap_or(uri);
            pieces.push(Piece {
                text: title.to_string(),
                style: Style {
                    link: Some(uri.to_string()),
                    ..Default::default()
                },
                raw: false,
            });
        } else if let Some(person) = e.get("person") {
            let props = person.get("personProperties");
            let email = props
                .and_then(|p| p.get("email"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let name = props
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(email);
            pieces.push(Piece {
                text: name.to_string(),
                style: Style {
                    link: Some(format!("mailto:{email}")),
                    ..Default::default()
                },
                raw: false,
            });
        } else if e.get("horizontalRule").is_some() {
            pieces.push(Piece {
                text: HR.to_string(),
                style: Style::default(),
                raw: true,
            });
        }
    }
    pieces
}

/// Wraps text in a marker, keeping surrounding spaces outside it (Markdown
/// does not allow `** bold**`).
fn wrap(text: &str, open: &str, close: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    format!("{}{open}{trimmed}{close}{}", &text[..start], &text[end..])
}

fn render_pieces(pieces: &[Piece], hard_break: &str) -> String {
    // Merge neighbours with the same style so markers are not repeated.
    let mut merged: Vec<(String, Style, bool)> = Vec::new();
    for p in pieces {
        let text = p.text.replace('\n', "");
        match merged.last_mut() {
            Some((t, s, raw)) if *s == p.style && !*raw && !p.raw => t.push_str(&text),
            _ => merged.push((text, p.style.clone(), p.raw)),
        }
    }
    let mut out = String::new();
    for (text, style, raw) in merged {
        if raw {
            if text != HR {
                out.push_str(&text);
            }
            continue;
        }
        let mut s = if style.code {
            let ticks = if text.contains('`') { "``" } else { "`" };
            wrap(&text, ticks, ticks)
        } else {
            escape_inline(&text)
        };
        if style.strike {
            s = wrap(&s, "~~", "~~");
        }
        if style.italic {
            s = wrap(&s, "*", "*");
        }
        if style.bold {
            s = wrap(&s, "**", "**");
        }
        if let Some(url) = &style.link {
            s = format!("[{s}]({})", url.replace(' ', "%20").replace(')', "%29"));
        }
        out.push_str(&s);
    }
    out.replace('\u{000b}', hard_break)
}

fn is_code_paragraph(pieces: &[Piece]) -> bool {
    let visible: Vec<&Piece> = pieces
        .iter()
        .filter(|p| !p.text.trim_matches(['\n', '\u{000b}']).is_empty())
        .collect();
    !visible.is_empty() && visible.iter().all(|p| p.style.code && !p.raw)
}

fn list_marker(doc: &Value, bullet: &Value, counters: &mut Vec<(String, usize, u32)>) -> String {
    let list_id = bullet.get("listId").and_then(Value::as_str).unwrap_or("");
    let level = bullet
        .get("nestingLevel")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let glyph = lists(doc)
        .and_then(|l| l.get(list_id))
        .and_then(|l| l.get("listProperties"))
        .and_then(|l| l.get("nestingLevels"))
        .and_then(Value::as_array)
        .and_then(|levels| levels.get(level))
        .and_then(|l| l.get("glyphType"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let ordered = ORDERED_GLYPHS.contains(&glyph);
    // Count items per (list, level); reset deeper levels when a shallower
    // item appears.
    counters.retain(|(id, l, _)| !(id == list_id && *l > level));
    let n = match counters
        .iter_mut()
        .find(|(id, l, _)| id == list_id && *l == level)
    {
        Some(c) => {
            c.2 += 1;
            c.2
        }
        None => {
            counters.push((list_id.to_string(), level, 1));
            1
        }
    };
    let indent = "   ".repeat(level);
    if ordered {
        format!("{indent}{n}. ")
    } else {
        format!("{indent}- ")
    }
}

fn table_to_markdown(doc: &Value, table: &Value) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for row in table
        .get("tableRows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut cells = Vec::new();
        for cell in row
            .get("tableCells")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let mut parts = Vec::new();
            for el in cell
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(p) = el.get("paragraph") {
                    let text = render_pieces(&paragraph_pieces(doc, p), "<br>");
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            }
            cells.push(parts.join("<br>").replace('|', "\\|"));
        }
        rows.push(cells);
    }
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let line = |cells: &[String]| {
        let mut padded: Vec<String> = cells.to_vec();
        padded.resize(cols, String::new());
        format!("| {} |", padded.join(" | "))
    };
    let mut out = vec![line(&rows[0]), format!("|{}", " --- |".repeat(cols))];
    for r in &rows[1..] {
        out.push(line(r));
    }
    out.join("\n")
}

/// Converts a `documents.get` response to Markdown.
pub(crate) fn document_to_markdown(doc: &Value) -> String {
    #[derive(PartialEq)]
    enum Kind {
        Text,
        List,
        Code,
    }
    let mut blocks: Vec<(Kind, String)> = Vec::new();
    let mut counters: Vec<(String, usize, u32)> = Vec::new();

    for el in body_content(doc) {
        if let Some(t) = el.get("table") {
            counters.clear();
            blocks.push((Kind::Text, table_to_markdown(doc, t)));
            continue;
        }
        let p = match el.get("paragraph") {
            Some(p) => p,
            None => continue,
        };
        let pieces = paragraph_pieces(doc, p);
        let is_rule = pieces.iter().any(|p| p.raw && p.text == HR)
            && pieces
                .iter()
                .all(|p| (p.raw && p.text == HR) || p.text.trim().is_empty());
        if is_rule {
            blocks.push((Kind::Text, "---".to_string()));
            continue;
        }
        let style = p
            .get("paragraphStyle")
            .and_then(|s| s.get("namedStyleType"))
            .and_then(Value::as_str)
            .unwrap_or("NORMAL_TEXT");

        if let Some(bullet) = p.get("bullet") {
            let marker = list_marker(doc, bullet, &mut counters);
            let text = render_pieces(&pieces, "<br>");
            blocks.push((Kind::List, format!("{marker}{}", text.trim())));
            continue;
        }
        counters.clear();

        if is_code_paragraph(&pieces) && !style.starts_with("HEADING") && style != "TITLE" {
            let text: String = pieces
                .iter()
                .map(|p| p.text.as_str())
                .collect::<String>()
                .trim_end_matches('\n')
                .replace('\u{000b}', "\n");
            match blocks.last_mut() {
                Some((Kind::Code, code)) => {
                    code.push('\n');
                    code.push_str(&text);
                }
                _ => blocks.push((Kind::Code, text)),
            }
            continue;
        }

        let text = render_pieces(&pieces, "\\\n");
        let text = text.trim_end().to_string();
        if text.trim().is_empty() {
            continue;
        }
        let prefix = match style {
            "TITLE" | "HEADING_1" => "# ",
            "SUBTITLE" | "HEADING_2" => "## ",
            "HEADING_3" => "### ",
            "HEADING_4" => "#### ",
            "HEADING_5" => "##### ",
            "HEADING_6" => "###### ",
            _ => "",
        };
        let text = if prefix.is_empty() {
            escape_line_start(&text)
        } else {
            text
        };
        blocks.push((Kind::Text, format!("{prefix}{text}")));
    }

    let mut out = String::new();
    let mut prev: Option<&Kind> = None;
    for (kind, text) in &blocks {
        if let Some(p) = prev {
            out.push_str(if *p == Kind::List && *kind == Kind::List {
                "\n"
            } else {
                "\n\n"
            });
        }
        if *kind == Kind::Code {
            let fence = if text.contains("```") { "~~~~" } else { "```" };
            out.push_str(&format!("{fence}\n{text}\n{fence}"));
        } else {
            out.push_str(text);
        }
        prev = Some(kind);
    }
    out.push('\n');
    out
}

/// Converts a `documents.get` response to plain text.
pub(crate) fn document_to_text(doc: &Value) -> String {
    fn walk(content: &[Value], out: &mut String) {
        for el in content {
            if let Some(p) = el.get("paragraph") {
                for e in p
                    .get("elements")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(t) = e
                        .get("textRun")
                        .and_then(|t| t.get("content"))
                        .and_then(Value::as_str)
                    {
                        out.push_str(&t.replace('\u{000b}', "\n"));
                    }
                }
            } else if let Some(t) = el.get("table") {
                for row in t
                    .get("tableRows")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let mut cells = Vec::new();
                    for cell in row
                        .get("tableCells")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        let mut s = String::new();
                        if let Some(c) = cell.get("content").and_then(Value::as_array) {
                            walk(c, &mut s);
                        }
                        cells.push(s.trim().replace('\n', " "));
                    }
                    out.push_str(&cells.join("\t"));
                    out.push('\n');
                }
            }
        }
    }
    let mut out = String::new();
    walk(body_content(doc), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(text: &str, style: Value) -> Value {
        json!({"textRun": {"content": text, "textStyle": style}})
    }

    fn para(elements: Vec<Value>, named: &str) -> Value {
        json!({"paragraph": {"elements": elements, "paragraphStyle": {"namedStyleType": named}}})
    }

    fn doc(content: Vec<Value>) -> Value {
        json!({
            "body": {"content": content},
            "lists": {
                "b": {"listProperties": {"nestingLevels": [{"glyphSymbol": "●"}, {"glyphSymbol": "○"}]}},
                "n": {"listProperties": {"nestingLevels": [{"glyphType": "DECIMAL"}, {"glyphType": "ALPHA"}]}}
            },
            "inlineObjects": {
                "img1": {"inlineObjectProperties": {"embeddedObject": {
                    "description": "logo",
                    "imageProperties": {"contentUri": "https://lh3.example/x", "sourceUri": ""}
                }}}
            }
        })
    }

    #[test]
    fn headings_and_inline_styles() {
        let d = doc(vec![
            para(vec![run("Plan\n", json!({}))], "HEADING_1"),
            para(
                vec![
                    run("Some ", json!({})),
                    run("bold ", json!({"bold": true})),
                    run("and italic", json!({"italic": true})),
                    run(", ", json!({})),
                    run(
                        "code",
                        json!({"weightedFontFamily": {"fontFamily": "Roboto Mono"}}),
                    ),
                    run(" and a ", json!({})),
                    run("link", json!({"link": {"url": "https://example.com"}})),
                    run(".\n", json!({})),
                ],
                "NORMAL_TEXT",
            ),
        ]);
        assert_eq!(
            document_to_markdown(&d),
            "# Plan\n\nSome **bold** *and italic*, `code` and a [link](https://example.com).\n"
        );
    }

    #[test]
    fn lists_nest_and_number() {
        let item = |t: &str, list: &str, level: u64| {
            json!({"paragraph": {
                "elements": [{"textRun": {"content": format!("{t}\n")}}],
                "bullet": {"listId": list, "nestingLevel": level}
            }})
        };
        let d = doc(vec![
            item("one", "b", 0),
            item("nested", "b", 1),
            item("two", "b", 0),
            para(vec![run("between\n", json!({}))], "NORMAL_TEXT"),
            item("first", "n", 0),
            item("sub", "n", 1),
            item("second", "n", 0),
        ]);
        assert_eq!(
            document_to_markdown(&d),
            "- one\n   - nested\n- two\n\nbetween\n\n1. first\n   1. sub\n2. second\n"
        );
    }

    #[test]
    fn tables_code_images_and_rules() {
        let cell = |t: &str| json!({"content": [{"paragraph": {"elements": [{"textRun": {"content": format!("{t}\n")}}]}}]});
        let mono = json!({"weightedFontFamily": {"fontFamily": "Courier New"}});
        let d = doc(vec![
            json!({"table": {"tableRows": [
                {"tableCells": [cell("Name"), cell("Count")]},
                {"tableCells": [cell("a|b"), cell("2")]}
            ]}}),
            para(vec![run("def f():\n", mono.clone())], "NORMAL_TEXT"),
            para(vec![run("    return 1\n", mono)], "NORMAL_TEXT"),
            para(
                vec![json!({"inlineObjectElement": {"inlineObjectId": "img1"}})],
                "NORMAL_TEXT",
            ),
            para(
                vec![json!({"horizontalRule": {}}), run("\n", json!({}))],
                "NORMAL_TEXT",
            ),
        ]);
        assert_eq!(
            document_to_markdown(&d),
            "| Name | Count |\n| --- | --- |\n| a\\|b | 2 |\n\n```\ndef f():\n    return 1\n```\n\n![logo](<https://lh3.example/x>)\n\n---\n"
        );
        assert_eq!(inline_image_uris(&d), vec!["https://lh3.example/x"]);
    }

    #[test]
    fn escapes_markdown_syntax_in_text() {
        let d = doc(vec![
            para(
                vec![run("1. not a list *really*\n", json!({}))],
                "NORMAL_TEXT",
            ),
            para(vec![run("# not a heading\n", json!({}))], "NORMAL_TEXT"),
        ]);
        assert_eq!(
            document_to_markdown(&d),
            "1\\. not a list \\*really\\*\n\n\\# not a heading\n"
        );
    }

    #[test]
    fn markers_hug_text_not_spaces() {
        assert_eq!(wrap(" bold ", "**", "**"), " **bold** ");
        assert_eq!(wrap("   ", "**", "**"), "   ");
    }

    #[test]
    fn plain_text_and_tabs_layout() {
        let cell = |t: &str| json!({"content": [{"paragraph": {"elements": [{"textRun": {"content": format!("{t}\n")}}]}}]});
        let d = json!({"tabs": [{"documentTab": {"body": {"content": [
            {"paragraph": {"elements": [{"textRun": {"content": "Hello\u{000b}world\n"}}]}},
            {"table": {"tableRows": [{"tableCells": [cell("a"), cell("b")]}]}}
        ]}}}]});
        assert_eq!(document_to_text(&d), "Hello\nworld\na\tb\n");
        assert_eq!(
            document_to_markdown(&d),
            "Hello\\\nworld\n\n| a | b |\n| --- | --- |\n"
        );
    }
}

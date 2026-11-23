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

//! Minimal HTML → plain text conversion for reading email bodies.
//!
//! Email HTML is mostly tables, paragraphs and links. This converter keeps the
//! readable text, turns block elements into line breaks, renders list items as
//! `- ` bullets, appends link targets in parentheses (so verification and
//! sign-in links survive), drops `<script>`, `<style>`, `<head>` and comments,
//! and decodes the common character entities. It is not a full HTML parser:
//! malformed markup degrades to "tags stripped" rather than failing.

/// Elements whose content is never shown to a reader.
const SKIPPED_ELEMENTS: &[&str] = &["script", "style", "head", "title", "noscript", "template"];

/// Elements that start or end a line.
const BLOCK_ELEMENTS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "center",
    "dd",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "tbody",
    "tfoot",
    "thead",
    "tr",
    "ul",
];

/// Convert an HTML document or fragment into readable plain text.
pub(super) fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    // Pending link targets, innermost last; each holds (href, text length at open).
    let mut links: Vec<(String, usize)> = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < html.len() {
        if bytes[i] != b'<' {
            let next = html[i..].find('<').map_or(html.len(), |off| i + off);
            push_text(&mut out, &decode_entities(&html[i..next]));
            i = next;
            continue;
        }

        // Comments: <!-- ... -->
        if html[i..].starts_with("<!--") {
            i = html[i + 4..]
                .find("-->")
                .map_or(html.len(), |off| i + 4 + off + 3);
            continue;
        }

        let Some(tag_end) = find_tag_end(html, i) else {
            // A stray '<' with no closing '>': treat the rest as text.
            push_text(&mut out, &decode_entities(&html[i..]));
            break;
        };
        let tag = &html[i + 1..tag_end];
        i = tag_end + 1;

        // Doctype, CDATA and processing instructions carry no text.
        if tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }

        let closing = tag.starts_with('/');
        let name = tag_name(tag.trim_start_matches('/'));
        if name.is_empty() {
            // Not a tag after all (e.g. "a < b"): keep it as text.
            push_text(
                &mut out,
                &decode_entities(&html[tag_end - tag.len() - 1..=tag_end]),
            );
            continue;
        }

        if !closing && SKIPPED_ELEMENTS.contains(&name.as_str()) {
            i = skip_element(html, i, &name);
            continue;
        }

        match name.as_str() {
            "br" => out.push('\n'),
            "li" if !closing => {
                ensure_line_start(&mut out);
                out.push_str("- ");
            }
            "li" => ensure_line_start(&mut out),
            "td" | "th" if closing => out.push(' '),
            "img" if !closing => {
                if let Some(alt) = attr_value(tag, "alt").filter(|a| !a.trim().is_empty()) {
                    push_text(&mut out, &decode_entities(&alt));
                }
            }
            "a" if !closing => {
                let href = attr_value(tag, "href").map(|h| decode_entities(&h));
                links.push((href.unwrap_or_default(), out.len()));
            }
            "a" => {
                if let Some((href, start)) = links.pop() {
                    append_link_target(&mut out, &href, start);
                }
            }
            n if BLOCK_ELEMENTS.contains(&n) => {
                ensure_line_start(&mut out);
                if !closing && n == "hr" {
                    out.push_str("---\n");
                }
            }
            _ => {}
        }
    }

    normalize_whitespace(&out)
}

/// Find the `>` that ends the tag opening at `start`, honouring quoted
/// attribute values. Returns `None` when the tag never closes.
fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (off, &b) in html.as_bytes()[start + 1..].iter().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Some(start + 1 + off),
            None => {}
        }
    }
    None
}

/// Lowercased element name at the start of a tag body (`a href=...` → `a`).
fn tag_name(tag: &str) -> String {
    let name: String = tag
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if tag.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name
    } else {
        String::new()
    }
}

/// Skip past the closing tag of a raw-content element such as `<style>`.
fn skip_element(html: &str, from: usize, name: &str) -> usize {
    let closing = format!("</{name}");
    let lower = html[from..].to_ascii_lowercase();
    match lower.find(&closing) {
        Some(off) => {
            let close_start = from + off;
            find_tag_end(html, close_start).map_or(html.len(), |end| end + 1)
        }
        None => html.len(),
    }
}

/// Extract an attribute value from a tag body, case-insensitively.
fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut search_from = 0;
    while let Some(off) = lower[search_from..].find(attr) {
        let pos = search_from + off;
        search_from = pos + attr.len();
        // Must be a whole attribute name: preceded by whitespace.
        if pos == 0 || !bytes[pos - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[pos + attr.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest
                .split(|c: char| c.is_ascii_whitespace() || c == '>')
                .next()
                .unwrap_or(""),
        };
        return Some(value.to_string());
    }
    None
}

/// After a link's text, append ` (href)` when the target adds information.
fn append_link_target(out: &mut String, href: &str, start: usize) {
    let href = href.trim();
    let is_web = href.starts_with("http://") || href.starts_with("https://");
    if !is_web {
        return;
    }
    let text = out.get(start..).unwrap_or("").trim();
    if text.is_empty() {
        push_text(out, href);
    } else if text != href && text.trim_end_matches('/') != href.trim_end_matches('/') {
        out.push_str(" (");
        out.push_str(href);
        out.push(')');
    }
}

/// Append text, collapsing runs of whitespace the way a browser would.
fn push_text(out: &mut String, text: &str) {
    for c in text.chars() {
        if c == '\u{a0}' {
            out.push(' ');
        } else if c.is_whitespace() {
            if !out.ends_with([' ', '\n']) && !out.is_empty() {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
}

fn ensure_line_start(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Trim every line, drop invisible format characters, and allow at most one
/// blank line in a row.
fn normalize_whitespace(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut blank_run = 0;
    for line in text.lines() {
        let cleaned: String = line
            .chars()
            .filter(|c| {
                !matches!(
                    c,
                    '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}' | '\u{34f}' | '\u{ad}'
                )
            })
            .collect();
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run > 1 || lines.is_empty() {
                continue;
            }
        } else {
            blank_run = 0;
        }
        lines.push(trimmed.to_string());
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Decode named and numeric character references.
pub(super) fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map(|off| off + 1);
        let (entity, consumed) = match end {
            Some(e) if rest.as_bytes()[e] == b';' => (&rest[1..e], e + 1),
            Some(e) => (&rest[1..e], e),
            None => (&rest[1..], rest.len()),
        };
        match decode_entity(entity) {
            Some(decoded) => {
                out.push_str(&decoded);
                rest = &rest[consumed..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(entity: &str) -> Option<String> {
    if let Some(num) = entity.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        return char::from_u32(code).map(String::from);
    }
    let decoded = match entity {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        "ensp" | "emsp" | "thinsp" => " ",
        "ndash" => "–",
        "mdash" => "—",
        "hellip" => "…",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "laquo" => "«",
        "raquo" => "»",
        "bull" | "middot" => "·",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "cent" => "¢",
        "deg" => "°",
        "times" => "×",
        "zwnj" | "zwj" | "shy" => "",
        _ => return None,
    };
    Some(decoded.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(html_to_text("Hello world"), "Hello world");
    }

    #[test]
    fn paragraphs_and_breaks_become_lines() {
        let html = "<p>First</p><p>Second<br>Third</p>";
        assert_eq!(html_to_text(html), "First\nSecond\nThird");
    }

    #[test]
    fn script_style_head_and_comments_are_dropped() {
        let html = "<html><head><title>T</title><style>p{color:#123456}</style></head>\
                    <body><!-- hidden 999999 --><script>var x = 1;</script><p>Visible</p></body></html>";
        assert_eq!(html_to_text(html), "Visible");
    }

    #[test]
    fn skipped_element_without_close_drops_rest() {
        assert_eq!(html_to_text("<p>A</p><style>p{}"), "A");
    }

    #[test]
    fn links_keep_their_target() {
        let html = r#"<p>Click <a href="https://example.com/verify?t=1&amp;u=2">here</a> now</p>"#;
        assert_eq!(
            html_to_text(html),
            "Click here (https://example.com/verify?t=1&u=2) now"
        );
    }

    #[test]
    fn link_whose_text_is_the_url_is_not_repeated() {
        let html = r#"<a href="https://example.com/">https://example.com</a>"#;
        assert_eq!(html_to_text(html), "https://example.com");
    }

    #[test]
    fn empty_link_text_shows_the_url() {
        let html = r#"<a href='https://example.com/x'><img src="logo.png"></a>"#;
        assert_eq!(html_to_text(html), "https://example.com/x");
    }

    #[test]
    fn non_web_links_are_not_appended() {
        let html = r#"<a href="mailto:a@example.com">Email us</a>"#;
        assert_eq!(html_to_text(html), "Email us");
    }

    #[test]
    fn list_items_become_bullets() {
        let html = "<ul><li>One</li><li>Two</li></ul>";
        assert_eq!(html_to_text(html), "- One\n- Two");
    }

    #[test]
    fn table_cells_are_separated() {
        let html = "<table><tr><td>Code</td><td>123456</td></tr><tr><td>Next</td></tr></table>";
        assert_eq!(html_to_text(html), "Code 123456\nNext");
    }

    #[test]
    fn img_alt_text_is_kept() {
        assert_eq!(html_to_text(r#"<img alt="Logo" src="x.png">"#), "Logo");
    }

    #[test]
    fn hr_renders_a_rule() {
        assert_eq!(html_to_text("A<hr>B"), "A\n---\nB");
    }

    #[test]
    fn whitespace_collapses_and_blank_lines_limited() {
        let html = "<div>  A \n\n  B  </div><p></p><p></p><p></p><div>C</div>";
        assert_eq!(html_to_text(html), "A B\nC");
    }

    #[test]
    fn quoted_gt_inside_attribute_does_not_end_tag() {
        let html = r#"<a title="a > b" href="https://e.com">x</a>"#;
        assert_eq!(html_to_text(html), "x (https://e.com)");
    }

    #[test]
    fn stray_lt_is_text() {
        assert_eq!(html_to_text("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
        assert_eq!(html_to_text("a <"), "a <");
    }

    #[test]
    fn doctype_is_ignored() {
        assert_eq!(html_to_text("<!DOCTYPE html><p>Hi</p>"), "Hi");
    }

    #[test]
    fn invisible_characters_are_removed() {
        assert_eq!(html_to_text("<p>A\u{200c}&zwnj;B&#8203;</p>"), "AB");
    }

    #[test]
    fn entities_decode() {
        assert_eq!(
            decode_entities("&lt;b&gt; &amp; &quot;q&quot; &#39;s&#x27; &mdash; &copy;"),
            "<b> & \"q\" 's' — ©"
        );
    }

    #[test]
    fn unknown_or_bare_entities_are_kept() {
        assert_eq!(decode_entities("AT&T &bogus; a & b"), "AT&T &bogus; a & b");
        assert_eq!(decode_entities("&amp"), "&");
        assert_eq!(decode_entities("&#xZZ;"), "&#xZZ;");
    }

    #[test]
    fn nbsp_becomes_space() {
        assert_eq!(html_to_text("A&nbsp;B"), "A B");
    }

    #[test]
    fn attr_value_handles_quoting_styles() {
        assert_eq!(attr_value(r#"a href="x""#, "href").as_deref(), Some("x"));
        assert_eq!(attr_value("a href='y'", "href").as_deref(), Some("y"));
        assert_eq!(attr_value("a HREF=z class=c", "href").as_deref(), Some("z"));
        assert_eq!(attr_value("a data-href=\"no\"", "href"), None);
        assert_eq!(attr_value("a href", "href"), None);
    }

    #[test]
    fn tag_name_requires_leading_letter() {
        assert_eq!(tag_name("DIV class=x"), "div");
        assert_eq!(tag_name("3"), "");
        assert_eq!(tag_name(" a"), "");
    }
}

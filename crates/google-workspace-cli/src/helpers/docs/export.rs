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

//! `docs +export`: a Google Doc as Markdown, plain text or HTML.
//!
//! Drive's own export does the conversion. Its Markdown embeds every image
//! as a base64 `data:` URI, which makes the file unreadable and unusable in
//! a repo, so images are turned into links: files saved next to `--output`,
//! or the image URLs from the Docs API when writing to stdout. When Drive
//! refuses the export (it caps exports at 10 MB), the document is converted
//! from the Docs API JSON instead.

use super::json_to_md;
use crate::error::GwsError;
use crate::helpers::drive::api::{error_reason, DriveClient, DOC_MIME};
use crate::helpers::drive::path::{resolve_target, Create, Expect};
use base64::Engine as _;
use std::path::{Path, PathBuf};

pub(crate) const DOCS_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/documents.readonly";
const DOCS_API: &str = "https://docs.googleapis.com/v1/documents";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    Markdown,
    Text,
    Html,
}

impl ExportFormat {
    pub fn parse(value: Option<&str>) -> Result<Self, GwsError> {
        match value.map(str::to_lowercase).as_deref() {
            None | Some("md") | Some("markdown") => Ok(Self::Markdown),
            Some("txt") | Some("text") => Ok(Self::Text),
            Some("html") => Ok(Self::Html),
            Some(other) => Err(GwsError::Validation(format!(
                "docs +export --format must be md, txt or html, got '{other}'"
            ))),
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown",
            Self::Text => "text/plain",
            Self::Html => "text/html",
        }
    }
}

/// A `data:` image found in Drive's Markdown export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DataImage {
    pub label: String,
    pub mime: String,
    pub base64: String,
}

/// Parses a reference definition line like
/// `[image1]: <data:image/png;base64,AAAA>`.
fn parse_data_definition(line: &str) -> Option<DataImage> {
    let rest = line.strip_prefix('[')?;
    let (label, rest) = rest.split_once("]:")?;
    let rest = rest.trim();
    let rest = rest.strip_prefix('<').unwrap_or(rest);
    let rest = rest.strip_suffix('>').unwrap_or(rest);
    let data = rest.strip_prefix("data:")?;
    let (mime, b64) = data.split_once(";base64,")?;
    Some(DataImage {
        label: label.to_string(),
        mime: mime.to_string(),
        base64: b64.trim().to_string(),
    })
}

/// Replaces each `data:` image definition with the link `link_for` returns
/// (given the image and its position). Returns the new Markdown and the
/// number of images seen.
pub(crate) fn rewrite_image_definitions(
    markdown: &str,
    mut link_for: impl FnMut(usize, &DataImage) -> Result<String, GwsError>,
) -> Result<(String, usize), GwsError> {
    let mut out = String::with_capacity(markdown.len());
    let mut n = 0;
    for line in markdown.split_inclusive('\n') {
        let (content, newline) = match line.strip_suffix('\n') {
            Some(c) => (c, "\n"),
            None => (line, ""),
        };
        match parse_data_definition(content) {
            Some(img) => {
                let link = link_for(n, &img)?;
                out.push_str(&format!("[{}]: <{}>{newline}", img.label, link));
                n += 1;
            }
            None => out.push_str(line),
        }
    }
    Ok((out, n))
}

fn is_list_item(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") "))
}

fn fence_marker(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.starts_with("```") {
        Some("```")
    } else if t.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

/// Removes escapes Markdown does not need, so exported text reads (and
/// diffs) like hand-written Markdown. Code spans and fences are untouched.
fn unescape_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let at_line_start = |out: &String| out.trim().is_empty();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            // Copy a code span verbatim.
            let run = chars[i..].iter().take_while(|&&x| x == '`').count();
            let mut j = i + run;
            let mut closed = None;
            while j < chars.len() {
                if chars[j] == '`' {
                    let r = chars[j..].iter().take_while(|&&x| x == '`').count();
                    if r == run {
                        closed = Some(j + r);
                        break;
                    }
                    j += r;
                } else {
                    j += 1;
                }
            }
            let end = closed.unwrap_or(i + run);
            out.extend(&chars[i..end]);
            i = end;
            continue;
        }
        if c == '\\' && i + 1 < chars.len() {
            let x = chars[i + 1];
            let prev = out.chars().last();
            let next = chars.get(i + 2).copied();
            let drop = match x {
                '_' => {
                    prev.is_some_and(char::is_alphanumeric)
                        && next.is_some_and(char::is_alphanumeric)
                }
                '*' => prev == Some(' ') && next == Some(' '),
                '.' | ')' => {
                    let t = out.trim_start();
                    !(!t.is_empty() && t.chars().all(|ch| ch.is_ascii_digit()))
                }
                '-' | '+' | '#' | '>' | '=' => !at_line_start(&out),
                '!' => !matches!(next, Some('[') | Some('\\')),
                _ => false,
            };
            if drop {
                out.push(x);
            } else {
                out.push(c);
                out.push(x);
            }
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `Some(true)` for a numbered item, `Some(false)` for a bullet.
fn list_kind(line: &str) -> Option<bool> {
    if !is_list_item(line) {
        return None;
    }
    Some(line.trim_start().starts_with(|c: char| c.is_ascii_digit()))
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// `| :---- | ----: |` → `| --- | ---: |` (left is the default alignment).
fn normalize_table_separator(line: &str) -> String {
    let cells: Vec<String> = line
        .trim()
        .trim_matches('|')
        .split('|')
        .map(|c| {
            let c = c.trim();
            match (c.starts_with(':'), c.ends_with(':') && c.len() > 1) {
                (true, true) => ":---:".to_string(),
                (false, true) => "---:".to_string(),
                _ => "---".to_string(),
            }
        })
        .collect();
    format!("| {} |", cells.join(" | "))
}

/// Turns `![alt][label]` references to the given definitions into inline
/// `![alt](link)` images and drops the definitions.
pub(crate) fn inline_image_references(markdown: &str, links: &[(String, String)]) -> String {
    let mut body = String::with_capacity(markdown.len());
    for line in markdown.split_inclusive('\n') {
        let is_definition = links
            .iter()
            .any(|(label, _)| line.starts_with(&format!("[{label}]: ")));
        if !is_definition {
            body.push_str(line);
        }
    }
    for (label, link) in links {
        let target = if link.contains([' ', '(', ')', '<', '>']) {
            format!("(<{link}>)")
        } else {
            format!("({link})")
        };
        body = body.replace(&format!("][{label}]"), &format!("]{target}"));
    }
    let trimmed = body.trim_end();
    format!("{trimmed}\n")
}

/// Tidies Drive's Markdown: drops the two-space line breaks it puts after
/// list items and before blank lines, and needless escapes.
pub(crate) fn tidy_markdown(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut fence: Option<&str> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(f) = fence {
            if line.trim_start().starts_with(f) {
                fence = None;
            }
            out.push(line.to_string());
            continue;
        }
        if let Some(f) = fence_marker(line) {
            fence = Some(f);
            out.push(line.to_string());
            continue;
        }
        if is_table_separator(line) {
            out.push(normalize_table_separator(line));
            continue;
        }
        let next = lines.get(i + 1).copied();
        let ends_block = next.is_none_or(|n| n.trim().is_empty() || is_list_item(n));
        let mut l = line.to_string();
        if l.ends_with("  ") && (is_list_item(line) || ends_block) {
            l = l.trim_end().to_string();
        }
        out.push(unescape_line(&l));
        // Drive drops the blank line between a bullet list and a numbered
        // list that follows it; put it back so the lists stay apart.
        if let Some(n) = next {
            if list_kind(line).is_some()
                && list_kind(n).is_some()
                && list_kind(line) != list_kind(n)
                && indent_of(line) == indent_of(n)
            {
                out.push(String::new());
            }
        }
    }
    let mut s = out.join("\n");
    if markdown.ends_with('\n') || !s.is_empty() {
        s.push('\n');
    }
    s
}

/// `notes.md` → `notes_images`.
pub(crate) fn images_dir_for(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    output.with_file_name(format!("{stem}_images"))
}

pub(crate) struct ExportResult {
    pub content: Vec<u8>,
    pub document_id: String,
    pub name: String,
    pub images: usize,
    pub converted_from_json: bool,
}

async fn docs_json(client: &DriveClient, id: &str) -> Result<serde_json::Value, GwsError> {
    let url = format!("{DOCS_API}/{}", crate::validate::encode_path_segment(id));
    client.get_json(&url, &[]).await
}

pub(crate) async fn run_export(
    client: &DriveClient,
    input: &str,
    format: ExportFormat,
    output: Option<&Path>,
) -> Result<ExportResult, GwsError> {
    let resolved = resolve_target(client, input, Expect::Mime(DOC_MIME), Create::No).await?;
    let id = resolved.file.id.clone();
    let doc_link = resolved
        .file
        .web_view_link
        .clone()
        .unwrap_or_else(|| format!("https://docs.google.com/document/d/{id}/edit"));

    let (exported, converted_from_json) = match client.export(&id, format.mime()).await {
        Ok(bytes) => (bytes, false),
        Err(e)
            if error_reason(&e) == Some("exportSizeLimitExceeded")
                && format != ExportFormat::Html =>
        {
            eprintln!("Drive will not export a document this large; converting from the Docs API instead.");
            let json = docs_json(client, &id).await?;
            let text = match format {
                ExportFormat::Markdown => json_to_md::document_to_markdown(&json),
                _ => json_to_md::document_to_text(&json),
            };
            (text.into_bytes(), true)
        }
        Err(e) => return Err(e),
    };

    // Drive's text export starts with a byte-order mark.
    let exported = match exported.strip_prefix("\u{feff}".as_bytes()) {
        Some(rest) => rest.to_vec(),
        None => exported,
    };

    // ...and uses CRLF line endings.
    let exported = if format == ExportFormat::Text {
        String::from_utf8_lossy(&exported)
            .replace("\r\n", "\n")
            .into_bytes()
    } else {
        exported
    };

    if format != ExportFormat::Markdown || converted_from_json {
        return Ok(ExportResult {
            content: exported,
            document_id: id,
            name: resolved.file.name,
            images: 0,
            converted_from_json,
        });
    }

    let markdown = String::from_utf8(exported).map_err(|e| {
        GwsError::Other(anyhow::anyhow!(
            "Drive returned Markdown that is not UTF-8: {e}"
        ))
    })?;

    let (markdown, images) = match output {
        Some(out_path) => {
            let dir = images_dir_for(out_path);
            let dir_name = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("images")
                .to_string();
            let mut created = false;
            let mut links = Vec::new();
            let (md, count) = rewrite_image_definitions(&markdown, |n, img| {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(img.base64.as_bytes())
                    .map_err(|e| {
                        GwsError::Other(anyhow::anyhow!("Image {} is not valid base64: {e}", n + 1))
                    })?;
                if !created {
                    std::fs::create_dir_all(&dir).map_err(|e| {
                        GwsError::Validation(format!("Failed to create '{}': {e}", dir.display()))
                    })?;
                    created = true;
                }
                let ext = crate::executor::mime_to_extension(&img.mime);
                let file = format!("image{}.{ext}", n + 1);
                std::fs::write(dir.join(&file), bytes).map_err(|e| {
                    GwsError::Validation(format!("Failed to write image '{file}': {e}"))
                })?;
                let link = format!("{dir_name}/{file}");
                links.push((img.label.clone(), link.clone()));
                Ok(link)
            })?;
            (inline_image_references(&md, &links), count)
        }
        None => {
            let (_, count) = rewrite_image_definitions(&markdown, |_, _| Ok(String::new()))?;
            let uris = if count > 0 {
                json_to_md::inline_image_uris(&docs_json(client, &id).await?)
            } else {
                vec![]
            };
            if count > 0 && uris.len() != count {
                eprintln!(
                    "warning: {count} images in the export but {} in the document; linking them to the document",
                    uris.len()
                );
            }
            let mut links = Vec::new();
            let (md, count) = rewrite_image_definitions(&markdown, |n, img| {
                let link = if uris.len() == count {
                    uris[n].clone()
                } else {
                    doc_link.clone()
                };
                links.push((img.label.clone(), link.clone()));
                Ok(link)
            })?;
            (inline_image_references(&md, &links), count)
        }
    };

    Ok(ExportResult {
        content: tidy_markdown(&markdown).into_bytes(),
        document_id: id,
        name: resolved.file.name,
        images,
        converted_from_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_formats() {
        assert_eq!(ExportFormat::parse(None).unwrap(), ExportFormat::Markdown);
        assert_eq!(
            ExportFormat::parse(Some("MD")).unwrap(),
            ExportFormat::Markdown
        );
        assert_eq!(
            ExportFormat::parse(Some("txt")).unwrap(),
            ExportFormat::Text
        );
        assert_eq!(
            ExportFormat::parse(Some("html")).unwrap(),
            ExportFormat::Html
        );
        assert!(ExportFormat::parse(Some("json")).is_err());
        assert!(ExportFormat::parse(Some("pdf")).is_err());
    }

    #[test]
    fn rewrites_data_images() {
        let md = "# T\n\n![logo][image1]\n\n[image1]: <data:image/png;base64,iVBORw0K>\n";
        let (out, n) = rewrite_image_definitions(md, |i, img| {
            assert_eq!(i, 0);
            assert_eq!(img.mime, "image/png");
            assert_eq!(img.base64, "iVBORw0K");
            Ok("t_images/image1.png".into())
        })
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            out,
            "# T\n\n![logo][image1]\n\n[image1]: <t_images/image1.png>\n"
        );
        // Ordinary reference links are left alone.
        let (same, n) =
            rewrite_image_definitions("[a]: https://x.org\n", |_, _| unreachable!()).unwrap();
        assert_eq!((same.as_str(), n), ("[a]: https://x.org\n", 0));
    }

    #[test]
    fn tidies_drive_markdown() {
        let md = "- one  \n- two  \n1. first  \n\nLine with a\\_snake\\_case word, 5 \\* 3\\.  \nnext\n\n1\\. keep\n\\- keep\nmid \\- drop \\# drop\n`a\\_b` \\!\\[x\\]\n```\ncode\\_x  \n```\n";
        assert_eq!(
            tidy_markdown(md),
            "- one\n- two\n\n1. first\n\nLine with a_snake_case word, 5 * 3.  \nnext\n\n1\\. keep\n\\- keep\nmid - drop # drop\n`a\\_b` \\!\\[x\\]\n```\ncode\\_x  \n```\n"
        );
    }

    #[test]
    fn inlines_image_references() {
        let md = "a ![logo][image1] b\n\n[image1]: <x_images/image1.png>\n[other]: https://o\n";
        let links = vec![("image1".to_string(), "x_images/image1.png".to_string())];
        assert_eq!(
            inline_image_references(md, &links),
            "a ![logo](x_images/image1.png) b\n\n[other]: https://o\n"
        );
        let spaced = vec![("image1".to_string(), "my images/1.png".to_string())];
        assert!(inline_image_references(md, &spaced).contains("![logo](<my images/1.png>)"));
    }

    #[test]
    fn tidies_tables_and_list_boundaries() {
        let md = "| A | B | C |\n| :---- | ----: | :--: |\n| 1 | 2 | 3 |\n\n- three  \n1. first  \n   - nested\n";
        assert_eq!(
            tidy_markdown(md),
            "| A | B | C |\n| --- | ---: | :---: |\n| 1 | 2 | 3 |\n\n- three\n\n1. first\n   - nested\n"
        );
    }

    #[test]
    fn images_dir_is_next_to_output() {
        assert_eq!(
            images_dir_for(Path::new("/w/docs/plan.md")),
            PathBuf::from("/w/docs/plan_images")
        );
    }
}

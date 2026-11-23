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

//! `docs +write <file.md>`: Markdown → a Google Doc with real formatting.
//!
//! Drive converts Markdown to a Doc on upload (headings, nested lists, bold,
//! italic, strikethrough, links, code as monospace, tables, images). The
//! helper uploads through that converter: `files.create` for a new Doc,
//! `files.update` with media to replace an existing Doc's body in place, so
//! its id, link, sharing and revision history stay.
//!
//! Drive fetches remote images itself but cannot see local files, so images
//! with a local path are inlined as `data:` URIs first.

use crate::error::GwsError;
use crate::helpers::drive::api::{DriveClient, DriveFile, Media, DOC_MIME};
use crate::helpers::drive::path::{resolve_target, Create, Expect};
use base64::Engine as _;
use serde_json::json;
use std::path::Path;

/// Image types Docs can hold.
const IMAGE_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
];

/// Drive will not convert a Markdown upload larger than this.
const MAX_IMPORT_BYTES: usize = 50 * 1024 * 1024;

fn image_mime(path: &str) -> Option<&'static str> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?
        .to_lowercase();
    IMAGE_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

fn is_remote(dest: &str) -> bool {
    dest.contains("://") || dest.starts_with("data:") || dest.starts_with("mailto:")
}

/// An inline image found in the Markdown source.
#[derive(Debug, PartialEq, Eq)]
struct ImageRef {
    /// Byte range of the whole `![alt](dest "title")`.
    start: usize,
    end: usize,
    alt: String,
    dest: String,
}

/// Finds `![alt](dest)` images on one line, skipping code spans.
fn find_images(line: &str) -> Vec<ImageRef> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'`' => {
                let run = bytes[i..].iter().take_while(|&&b| b == b'`').count();
                let fence = &line[i..i + run];
                match line[i + run..].find(fence) {
                    Some(off) => i += run + off + run,
                    None => i += run,
                }
            }
            b'\\' => i += 2,
            b'!' if bytes.get(i + 1) == Some(&b'[') => {
                if let Some(img) = parse_image_at(line, i) {
                    i = img.end;
                    found.push(img);
                } else {
                    i += 2;
                }
            }
            _ => i += 1,
        }
    }
    found
}

fn parse_image_at(line: &str, start: usize) -> Option<ImageRef> {
    let rest = &line[start + 2..];
    let close = rest.find("](")?;
    let alt = &rest[..close];
    if alt.contains('[') {
        return None;
    }
    let after = &rest[close + 2..];
    let (dest, consumed) = if let Some(inner) = after.strip_prefix('<') {
        let end = inner.find('>')?;
        (&inner[..end], end + 2)
    } else {
        let end = after
            .find(|c: char| c.is_whitespace() || c == ')')
            .unwrap_or(after.len());
        (&after[..end], end)
    };
    // Skip an optional title, then the closing parenthesis.
    let tail = &after[consumed..];
    let close_paren = tail.find(')')?;
    let between = tail[..close_paren].trim();
    if !(between.is_empty()
        || (between.starts_with('"') && between.ends_with('"'))
        || (between.starts_with('\'') && between.ends_with('\'')))
    {
        return None;
    }
    let end = start + 2 + close + 2 + consumed + close_paren + 1;
    Some(ImageRef {
        start,
        end,
        alt: alt.to_string(),
        dest: dest.to_string(),
    })
}

/// The Markdown to upload, with local images inlined.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub markdown: String,
    pub images_inlined: Vec<String>,
    pub warnings: Vec<String>,
}

/// Inlines local images as `data:` reference definitions. `read` loads an
/// image given its path as written, relative to the Markdown file.
pub(crate) fn inline_local_images(
    markdown: &str,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, GwsError>,
) -> Result<Prepared, GwsError> {
    let mut out = String::with_capacity(markdown.len());
    let mut definitions = String::new();
    let mut inlined = Vec::new();
    let mut warnings = Vec::new();
    let mut fence: Option<&str> = None;

    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(f) = fence {
            if trimmed.starts_with(f) {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(&trimmed[..3]);
            out.push_str(line);
            continue;
        }
        let mut cursor = 0;
        for img in find_images(line) {
            if is_remote(&img.dest) {
                continue;
            }
            let decoded = percent_encoding::percent_decode_str(&img.dest)
                .decode_utf8_lossy()
                .to_string();
            let Some(mime) = image_mime(&decoded) else {
                warnings.push(format!(
                    "{}: not a PNG, JPEG, GIF, WebP or BMP image; left as a link",
                    img.dest
                ));
                continue;
            };
            let bytes = read(&decoded)?;
            let label = format!("gws-image-{}", inlined.len() + 1);
            out.push_str(&line[cursor..img.start]);
            out.push_str(&format!("![{}][{label}]", img.alt));
            cursor = img.end;
            definitions.push_str(&format!(
                "\n[{label}]: <data:{mime};base64,{}>",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ));
            inlined.push(decoded);
        }
        out.push_str(&line[cursor..]);
    }
    if !definitions.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&definitions);
        out.push('\n');
    }
    Ok(Prepared {
        markdown: out,
        images_inlined: inlined,
        warnings,
    })
}

/// Where the Markdown goes.
pub(crate) enum Destination<'a> {
    /// Replace the body of this Doc (id, URL or Drive path).
    Existing(&'a str),
    /// Create a Doc with this title, in this folder (id, URL or path).
    New {
        title: &'a str,
        folder: Option<&'a str>,
    },
}

pub(crate) async fn run_write(
    client: &DriveClient,
    prepared: &Prepared,
    dest: Destination<'_>,
) -> Result<(DriveFile, &'static str), GwsError> {
    let bytes = prepared.markdown.as_bytes();
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(GwsError::Validation(format!(
            "The Markdown (with inlined images) is {} MB; Drive converts at most 50 MB",
            bytes.len() / (1024 * 1024)
        )));
    }
    match dest {
        Destination::Existing(target) => {
            let doc = resolve_target(client, target, Expect::Mime(DOC_MIME), Create::No).await?;
            let file = client
                .update_media(&doc.file.id, Media::Bytes(bytes), "text/markdown")
                .await?;
            Ok((file, "replaced"))
        }
        Destination::New { title, folder } => {
            let mut metadata = json!({ "name": title, "mimeType": DOC_MIME });
            if let Some(folder) = folder {
                let f = resolve_target(client, folder, Expect::Folder, Create::No).await?;
                metadata["parents"] = json!([f.file.id]);
            }
            let file = client
                .create_with_media(&metadata, Media::Bytes(bytes), "text/markdown")
                .await?;
            Ok((file, "created"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_images_and_skips_code() {
        let line =
            "a ![x](img/a.png) `![no](b.png)` ![y](<my pic.jpg> \"T\") ![z](https://h/c.png)\n";
        let found = find_images(line);
        let dests: Vec<&str> = found.iter().map(|i| i.dest.as_str()).collect();
        assert_eq!(dests, vec!["img/a.png", "my pic.jpg", "https://h/c.png"]);
        assert_eq!(&line[found[0].start..found[0].end], "![x](img/a.png)");
        assert_eq!(
            &line[found[1].start..found[1].end],
            "![y](<my pic.jpg> \"T\")"
        );
        assert!(find_images("\\![x](a.png) [link](a.png) ![broken](a.png").is_empty());
    }

    #[test]
    fn inlines_local_images_only() {
        let md = "# T\n\n![logo](img/logo.png)\n\n![remote](https://x.org/r.png)\n\n```\n![in code](a.png)\n```\n![doc](notes.pdf)\n![sp](my%20pic.gif)";
        let mut asked = Vec::new();
        let p = inline_local_images(md, |path| {
            asked.push(path.to_string());
            Ok(vec![1, 2, 3])
        })
        .unwrap();
        assert_eq!(asked, vec!["img/logo.png", "my pic.gif"]);
        assert_eq!(p.images_inlined, asked);
        assert!(p.markdown.contains("![logo][gws-image-1]"));
        assert!(p.markdown.contains("![sp][gws-image-2]"));
        assert!(p.markdown.contains("![remote](https://x.org/r.png)"));
        assert!(p.markdown.contains("![in code](a.png)"));
        assert!(p.markdown.contains("![doc](notes.pdf)"));
        assert!(p
            .markdown
            .ends_with("\n[gws-image-1]: <data:image/png;base64,AQID>\n[gws-image-2]: <data:image/gif;base64,AQID>\n"));
        assert_eq!(p.warnings.len(), 1);
    }

    #[test]
    fn markdown_without_local_images_is_unchanged() {
        let md = "# Title\n\nText with **bold**.\n";
        let p = inline_local_images(md, |_| unreachable!()).unwrap();
        assert_eq!(p.markdown, md);
        assert!(p.images_inlined.is_empty());
    }

    #[test]
    fn read_errors_propagate() {
        let err = inline_local_images("![a](../../etc/x.png)", |_| {
            Err(GwsError::Validation("outside".into()))
        })
        .unwrap_err();
        assert!(err.to_string().contains("outside"));
    }

    #[test]
    fn image_mime_by_extension() {
        assert_eq!(image_mime("a.PNG"), Some("image/png"));
        assert_eq!(image_mime("a.jpeg"), Some("image/jpeg"));
        assert_eq!(image_mime("a.svg"), None);
        assert_eq!(image_mime("noext"), None);
    }
}

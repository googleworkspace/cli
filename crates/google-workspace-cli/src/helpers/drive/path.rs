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

//! Drive paths: `a/b/c` → file id, one folder name at a time.
//!
//! Syntax:
//! - `a/b/c`, `/a/b/c` and `My Drive/a/b/c` start at the root of My Drive.
//! - `Shared drives/<drive name>/a/b` starts at the root of a shared drive.
//! - `\/` is a literal slash inside a name; `\\` is a literal backslash.
//!
//! Wherever a command takes "an id or a path", a Drive or Docs URL and
//! `id:<ID>` are accepted as ids, and so is a single segment that looks like
//! a Drive id (25+ characters of `[A-Za-z0-9_-]`, no spaces).
//!
//! When two items in a folder share a name, resolution stops and lists them.
//! It never picks one.

use super::api::{DriveClient, DriveFile, FOLDER_MIME};
use crate::error::GwsError;
use crate::output::sanitize_for_terminal;
use serde::Serialize;

const MY_DRIVE: &str = "My Drive";
const SHARED_DRIVES: &str = "Shared drives";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DriveRoot {
    MyDrive,
    SharedDrive(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DrivePath {
    pub root: DriveRoot,
    pub segments: Vec<String>,
}

impl DrivePath {
    /// The path as the user would write it, rooted (for messages).
    pub fn display_prefix(&self, n: usize) -> String {
        let mut parts: Vec<String> = match &self.root {
            DriveRoot::MyDrive => vec![MY_DRIVE.to_string()],
            DriveRoot::SharedDrive(d) => vec![SHARED_DRIVES.to_string(), escape_segment(d)],
        };
        parts.extend(self.segments.iter().take(n).map(|s| escape_segment(s)));
        parts.join("/")
    }
}

/// An id, or a path still to resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Id(String),
    Path(DrivePath),
}

/// What the last segment of a path must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect<'a> {
    Any,
    Folder,
    Mime(&'a str),
}

impl Expect<'_> {
    fn accepts(&self, f: &DriveFile) -> bool {
        match self {
            Expect::Any => true,
            Expect::Folder => f.mime_type == FOLDER_MIME,
            Expect::Mime(m) => f.mime_type == *m,
        }
    }

    fn describe(&self) -> String {
        match self {
            Expect::Any => "item".to_string(),
            Expect::Folder => "folder".to_string(),
            Expect::Mime(m) => format!("file of type {m}"),
        }
    }
}

/// What to do about missing folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Create {
    No,
    Yes,
    /// Report what would be created, create nothing.
    DryRun,
}

/// The result of resolving a path.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Resolved {
    /// The final item. In a dry run that would create it, `id` is empty.
    pub file: DriveFile,
    /// The path, normalised (`My Drive/…` or `Shared drives/<name>/…`).
    pub path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub would_create: Vec<String>,
}

fn escape_segment(s: &str) -> String {
    s.replace('\\', "\\\\").replace('/', "\\/")
}

/// Splits a path into segments, honouring `\/` and `\\` escapes. Empty
/// segments (from a leading, trailing or doubled slash) are dropped.
fn split_segments(input: &str) -> Result<Vec<String>, GwsError> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(n @ ('/' | '\\')) => current.push(n),
                Some(n) => {
                    current.push('\\');
                    current.push(n);
                }
                None => current.push('\\'),
            },
            '/' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    Ok(segments)
}

/// Parses a Drive path. See the module docs for the syntax.
pub(crate) fn parse_drive_path(input: &str) -> Result<DrivePath, GwsError> {
    crate::validate::reject_dangerous_chars(input, "path")?;
    let mut segments = split_segments(input.trim())?;
    for s in &segments {
        if s == "." || s == ".." {
            return Err(GwsError::Validation(format!(
                "Drive path '{input}' contains '{s}'; Drive paths have no relative segments"
            )));
        }
    }
    let root = match segments.first().map(String::as_str) {
        Some(MY_DRIVE) => {
            segments.remove(0);
            DriveRoot::MyDrive
        }
        Some(SHARED_DRIVES) => {
            if segments.len() < 2 {
                return Err(GwsError::Validation(format!(
                    "Drive path '{input}' names no shared drive; use 'Shared drives/<drive name>/…'"
                )));
            }
            segments.remove(0);
            DriveRoot::SharedDrive(segments.remove(0))
        }
        _ => DriveRoot::MyDrive,
    };
    Ok(DrivePath { root, segments })
}

/// A Drive id: 25 or more characters of `[A-Za-z0-9_-]`.
pub(crate) fn looks_like_id(s: &str) -> bool {
    s.len() >= 25
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Extracts the file id from a Drive, Docs, Sheets or Slides URL.
pub(crate) fn id_from_url(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if !(host.ends_with("drive.google.com") || host.ends_with("docs.google.com")) {
        return None;
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    // …/d/<id>/… (files, docs, sheets, slides) or …/folders/<id>
    for w in parts.windows(2) {
        if (w[0] == "d" || w[0] == "folders") && looks_like_id_lenient(w[1]) {
            return Some(w[1].to_string());
        }
    }
    // open?id=<id> or uc?id=<id>
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, v)| *k == "id" && looks_like_id_lenient(v))
        .map(|(_, v)| v.to_string())
}

fn looks_like_id_lenient(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parses "an id, URL or path" as given on the command line.
pub(crate) fn parse_target(input: &str) -> Result<Target, GwsError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(GwsError::Validation(
            "Expected a Drive id, URL or path, got an empty string".to_string(),
        ));
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return id_from_url(trimmed).map(Target::Id).ok_or_else(|| {
            GwsError::Validation(format!("Could not find a Drive file id in URL '{trimmed}'"))
        });
    }
    if let Some(id) = trimmed.strip_prefix("id:") {
        crate::validate::validate_resource_name(id)?;
        return Ok(Target::Id(id.to_string()));
    }
    if !trimmed.contains('/') && looks_like_id(trimmed) {
        return Ok(Target::Id(trimmed.to_string()));
    }
    parse_drive_path(trimmed).map(Target::Path)
}

/// Lists ambiguous matches for an error message.
fn ambiguity_error(where_: &str, name: &str, matches: &[DriveFile]) -> GwsError {
    let mut msg = format!(
        "'{}' is ambiguous: {} items named '{}' in {}. Pass one of these ids instead:",
        sanitize_for_terminal(name),
        matches.len(),
        sanitize_for_terminal(name),
        sanitize_for_terminal(where_)
    );
    for m in matches {
        msg.push_str(&format!(
            "\n  {}  {}  {}",
            m.id,
            m.mime_type,
            m.modified_time.as_deref().unwrap_or("")
        ));
    }
    GwsError::Validation(msg)
}

/// Picks the single match for one segment, or explains why there isn't one.
/// `intermediate` segments must be folders.
pub(crate) fn pick_match(
    candidates: Vec<DriveFile>,
    expect: Expect<'_>,
    where_: &str,
    name: &str,
) -> Result<Option<DriveFile>, GwsError> {
    let accepted: Vec<DriveFile> = candidates
        .iter()
        .filter(|f| expect.accepts(f))
        .cloned()
        .collect();
    match accepted.len() {
        0 if candidates.is_empty() => Ok(None),
        0 => Err(GwsError::Validation(format!(
            "'{}' in {} is not a {} (found {})",
            sanitize_for_terminal(name),
            sanitize_for_terminal(where_),
            expect.describe(),
            candidates
                .iter()
                .map(|c| format!("{} {}", c.id, c.mime_type))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
        1 => Ok(accepted.into_iter().next()),
        _ => Err(ambiguity_error(where_, name, &accepted)),
    }
}

/// Resolves a path to an item, one folder name at a time.
pub(crate) async fn resolve_path(
    client: &DriveClient,
    path: &DrivePath,
    expect: Expect<'_>,
    create: Create,
) -> Result<Resolved, GwsError> {
    let (mut parent, drive_id) = match &path.root {
        DriveRoot::MyDrive => (client.get_file("root").await?, None),
        DriveRoot::SharedDrive(name) => {
            let drives = client.shared_drives_named(name).await?;
            match drives.len() {
                0 => {
                    return Err(GwsError::Validation(format!(
                        "No shared drive named '{}' is visible to this account",
                        sanitize_for_terminal(name)
                    )))
                }
                1 => {
                    let d = &drives[0];
                    let root = DriveFile {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        mime_type: FOLDER_MIME.to_string(),
                        drive_id: Some(d.id.clone()),
                        ..Default::default()
                    };
                    (root, Some(d.id.clone()))
                }
                _ => {
                    let mut msg = format!(
                        "{} shared drives are named '{}'. Pass a folder id instead:",
                        drives.len(),
                        sanitize_for_terminal(name)
                    );
                    for d in &drives {
                        msg.push_str(&format!("\n  {}", d.id));
                    }
                    return Err(GwsError::Validation(msg));
                }
            }
        }
    };

    if path.segments.is_empty() && !expect.accepts(&parent) {
        return Err(GwsError::Validation(format!(
            "{} is a folder, not a {}",
            path.display_prefix(0),
            expect.describe()
        )));
    }

    let mut created = Vec::new();
    let mut would_create = Vec::new();
    let last = path.segments.len().saturating_sub(1);
    for (i, name) in path.segments.iter().enumerate() {
        let where_ = path.display_prefix(i);
        let here = path.display_prefix(i + 1);
        let seg_expect = if i == last { expect } else { Expect::Folder };

        // Once a segment is missing in a dry run, everything below it is too.
        if !would_create.is_empty() {
            would_create.push(here);
            continue;
        }

        let candidates = client
            .children_named(&parent.id, name, drive_id.as_deref())
            .await?;
        match pick_match(candidates, seg_expect, &where_, name)? {
            Some(found) => parent = found,
            None => {
                let can_create = matches!(seg_expect, Expect::Folder | Expect::Any);
                match create {
                    Create::Yes if can_create => {
                        parent = client.create_folder(name, &parent.id).await?;
                        created.push(here);
                    }
                    Create::DryRun if can_create => {
                        would_create.push(here);
                    }
                    _ => {
                        return Err(GwsError::Validation(format!(
                            "Not found: no '{}' in {}{}",
                            sanitize_for_terminal(name),
                            sanitize_for_terminal(&where_),
                            if can_create && create == Create::No {
                                " (pass --create to make missing folders)"
                            } else {
                                ""
                            }
                        )))
                    }
                }
            }
        }
    }

    let file = if would_create.is_empty() {
        parent
    } else {
        DriveFile {
            id: String::new(),
            name: path.segments.last().cloned().unwrap_or_default(),
            mime_type: FOLDER_MIME.to_string(),
            ..Default::default()
        }
    };
    Ok(Resolved {
        file,
        path: path.display_prefix(path.segments.len()),
        created,
        would_create,
    })
}

/// Resolves an id, URL or path to an item, checking its type.
pub(crate) async fn resolve_target(
    client: &DriveClient,
    input: &str,
    expect: Expect<'_>,
    create: Create,
) -> Result<Resolved, GwsError> {
    match parse_target(input)? {
        Target::Id(id) => {
            let file = client.get_file(&id).await?;
            if !expect.accepts(&file) {
                return Err(GwsError::Validation(format!(
                    "{} ('{}') is a {}, not a {}",
                    file.id,
                    sanitize_for_terminal(&file.name),
                    file.mime_type,
                    expect.describe()
                )));
            }
            if file.trashed {
                return Err(GwsError::Validation(format!(
                    "{} ('{}') is in the trash",
                    file.id,
                    sanitize_for_terminal(&file.name)
                )));
            }
            Ok(Resolved {
                path: file.id.clone(),
                file,
                created: vec![],
                would_create: vec![],
            })
        }
        Target::Path(p) => resolve_path(client, &p, expect, create).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, mime: &str) -> DriveFile {
        DriveFile {
            id: id.to_string(),
            name: "x".to_string(),
            mime_type: mime.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn parses_my_drive_paths() {
        for input in ["a/b/c", "/a/b/c", "My Drive/a/b/c", "a//b/c/"] {
            let p = parse_drive_path(input).unwrap();
            assert_eq!(p.root, DriveRoot::MyDrive, "{input}");
            assert_eq!(p.segments, vec!["a", "b", "c"], "{input}");
        }
        let root = parse_drive_path("My Drive").unwrap();
        assert!(root.segments.is_empty());
    }

    #[test]
    fn parses_shared_drive_paths() {
        let p = parse_drive_path("Shared drives/Company/design/social").unwrap();
        assert_eq!(p.root, DriveRoot::SharedDrive("Company".into()));
        assert_eq!(p.segments, vec!["design", "social"]);
        assert!(parse_drive_path("Shared drives").is_err());
    }

    #[test]
    fn honours_escapes() {
        let p = parse_drive_path(r"Reports/Q1\/Q2 plan/a\\b").unwrap();
        assert_eq!(p.segments, vec!["Reports", "Q1/Q2 plan", r"a\b"]);
        assert_eq!(p.display_prefix(3), r"My Drive/Reports/Q1\/Q2 plan/a\\b");
    }

    #[test]
    fn rejects_relative_segments_and_control_chars() {
        assert!(parse_drive_path("a/../b").is_err());
        assert!(parse_drive_path("./a").is_err());
        assert!(parse_drive_path("a/b\u{0}c").is_err());
        assert!(parse_drive_path("a/\u{202e}b").is_err());
    }

    #[test]
    fn recognises_ids() {
        assert!(looks_like_id("1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf"));
        assert!(!looks_like_id("Posters"));
        assert!(!looks_like_id("a folder name with spaces in it ok"));
        assert_eq!(
            parse_target("1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf").unwrap(),
            Target::Id("1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf".into())
        );
        assert_eq!(parse_target("id:abc").unwrap(), Target::Id("abc".into()));
        assert!(parse_target("id:../x").is_err());
        assert!(matches!(parse_target("Posters").unwrap(), Target::Path(_)));
        assert!(matches!(
            parse_target("a/1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf").unwrap(),
            Target::Path(_)
        ));
        assert!(parse_target("  ").is_err());
    }

    #[test]
    fn extracts_ids_from_urls() {
        let cases = [
            "https://drive.google.com/drive/folders/1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf",
            "https://drive.google.com/drive/u/0/folders/1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf?usp=sharing",
            "https://drive.google.com/file/d/1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf/view",
            "https://docs.google.com/document/d/1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf/edit#heading=h.1",
            "https://drive.google.com/open?id=1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf",
        ];
        for url in cases {
            assert_eq!(
                id_from_url(url).as_deref(),
                Some("1T7pr7z_USxPVShCwkP3frWYukwJ6DjOf"),
                "{url}"
            );
        }
        assert_eq!(id_from_url("https://example.com/d/abc"), None);
        assert!(parse_target("https://example.com/d/abc").is_err());
    }

    #[test]
    fn pick_match_never_guesses() {
        let two = vec![file("1", FOLDER_MIME), file("2", FOLDER_MIME)];
        let err = pick_match(two, Expect::Folder, "My Drive", "x").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ambiguous") && msg.contains("  1  ") && msg.contains("  2  "));

        // A folder and a file share the name; an intermediate segment wants the folder.
        let mixed = vec![file("1", FOLDER_MIME), file("2", "image/png")];
        assert_eq!(
            pick_match(mixed.clone(), Expect::Folder, "My Drive", "x")
                .unwrap()
                .unwrap()
                .id,
            "1"
        );
        assert!(pick_match(mixed, Expect::Any, "My Drive", "x").is_err());

        assert!(pick_match(vec![], Expect::Any, "My Drive", "x")
            .unwrap()
            .is_none());
        let wrong = vec![file("3", "image/png")];
        assert!(pick_match(wrong, Expect::Folder, "My Drive", "x")
            .unwrap_err()
            .to_string()
            .contains("not a folder"));
    }
}

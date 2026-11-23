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

//! `drive +sync`: make a Drive folder match a local directory.
//!
//! The command works in two phases. It first builds a complete [`Plan`] from
//! the local tree and the remote tree, and only then touches Drive. If the
//! plan finds anything it cannot decide (two remote items with one name, a
//! file where a folder should be, a Google Doc where a file should be), it
//! lists every such conflict and changes nothing.
//!
//! Changed files are updated in place (`files.update` with media), so their
//! ids and share links survive. `--delete` moves extra items to the trash;
//! nothing is ever deleted permanently, and Google-native files (Docs,
//! Sheets, …), which have no local form, are left alone.

use super::api::{DriveClient, DriveFile, Media};
use super::path::{resolve_target, Create, Expect};
use crate::error::GwsError;
use crate::output::sanitize_for_terminal;
use futures_util::StreamExt;
use md5::{Digest, Md5};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Uploads in flight at once.
const CONCURRENCY: usize = 4;

/// Names skipped unless the user excludes something else: OS litter.
pub(crate) const DEFAULT_EXCLUDES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini", ".git"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalFile {
    pub abs: PathBuf,
    pub size: u64,
}

/// The local side: relative paths use `/` on every platform.
#[derive(Debug, Default)]
pub(crate) struct LocalTree {
    pub files: BTreeMap<String, LocalFile>,
    pub dirs: BTreeSet<String>,
    /// Symlinks and other non-regular entries, which are not followed.
    pub skipped: Vec<String>,
}

/// The remote side: every item found, keyed by relative path. A path with
/// more than one item is a name clash.
#[derive(Debug, Default)]
pub(crate) struct RemoteTree {
    pub items: BTreeMap<String, Vec<DriveFile>>,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Plan {
    pub create_folders: Vec<String>,
    /// Remote folders that already match a local directory: path → id.
    pub folders: BTreeMap<String, String>,
    pub upload: Vec<String>,
    /// Changed files: path → id.
    pub update: BTreeMap<String, String>,
    /// Identical files: path → id.
    pub unchanged: BTreeMap<String, String>,
    /// Items not present locally, to trash with `--delete`: path → id.
    pub trash: BTreeMap<String, String>,
    /// Google-native items not present locally, which `--delete` leaves.
    pub kept: BTreeMap<String, String>,
    pub conflicts: Vec<String>,
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
}

/// Matches a glob with `*` (any run of characters) and `?` (one character).
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// A pattern with a `/` matches the relative path; otherwise the name.
pub(crate) fn is_excluded(rel: &str, name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        if p.contains('/') {
            glob_match(p, rel)
        } else {
            glob_match(p, name)
        }
    })
}

/// Walks `root` recursively. Symlinks are listed as skipped, never followed.
pub(crate) fn walk_local(root: &Path, excludes: &[String]) -> Result<LocalTree, GwsError> {
    let mut tree = LocalTree::default();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel_dir)) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            GwsError::Validation(format!("Failed to read '{}': {e}", dir.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                GwsError::Validation(format!("Failed to read '{}': {e}", dir.display()))
            })?;
            let name = entry
                .file_name()
                .to_str()
                .map(str::to_string)
                .ok_or_else(|| {
                    GwsError::Validation(format!(
                        "File name is not valid UTF-8 in '{}': {:?}",
                        dir.display(),
                        entry.file_name()
                    ))
                })?;
            let rel = join(&rel_dir, &name);
            if is_excluded(&rel, &name, excludes) {
                continue;
            }
            let ft = entry.file_type().map_err(|e| {
                GwsError::Validation(format!("Failed to stat '{}': {e}", entry.path().display()))
            })?;
            if ft.is_dir() {
                tree.dirs.insert(rel.clone());
                stack.push((entry.path(), rel));
            } else if ft.is_file() {
                let size = entry
                    .metadata()
                    .map_err(|e| {
                        GwsError::Validation(format!(
                            "Failed to stat '{}': {e}",
                            entry.path().display()
                        ))
                    })?
                    .len();
                tree.files.insert(
                    rel,
                    LocalFile {
                        abs: entry.path(),
                        size,
                    },
                );
            } else {
                tree.skipped.push(rel);
            }
        }
    }
    tree.skipped.sort();
    Ok(tree)
}

/// MD5 of a file, hex-encoded, as Drive reports it in `md5Checksum`.
pub(crate) fn md5_file(path: &Path) -> Result<String, GwsError> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| GwsError::Validation(format!("Failed to open '{}': {e}", path.display())))?;
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| {
            GwsError::Validation(format!("Failed to read '{}': {e}", path.display()))
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Lists the remote folder. It only descends into folders that match a
/// local directory: anything under another folder is either untouched or
/// trashed with that folder.
pub(crate) async fn walk_remote(
    client: &DriveClient,
    root_id: &str,
    drive_id: Option<&str>,
    local_dirs: &BTreeSet<String>,
) -> Result<RemoteTree, GwsError> {
    let mut tree = RemoteTree::default();
    let mut queue = vec![(root_id.to_string(), String::new())];
    while let Some((folder_id, rel_dir)) = queue.pop() {
        for child in client.children(&folder_id, drive_id).await? {
            let rel = join(&rel_dir, &child.name);
            if child.is_folder() && local_dirs.contains(&rel) {
                queue.push((child.id.clone(), rel.clone()));
            }
            tree.items.entry(rel).or_default().push(child);
        }
    }
    Ok(tree)
}

/// Decides what to do with every path. `md5_of` is called only for local
/// files whose remote twin has the same size.
pub(crate) fn plan(
    local: &LocalTree,
    remote: &RemoteTree,
    delete: bool,
    mut md5_of: impl FnMut(&Path) -> Result<String, GwsError>,
) -> Result<Plan, GwsError> {
    let mut plan = Plan::default();
    let clash = |path: &str, items: &[DriveFile]| {
        format!(
            "{path}: {} remote items share this name ({})",
            items.len(),
            items
                .iter()
                .map(|i| format!("{} {}", i.id, i.mime_type))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    for dir in &local.dirs {
        match remote.items.get(dir).map(Vec::as_slice) {
            None | Some([]) => plan.create_folders.push(dir.clone()),
            Some([one]) if one.is_folder() => {
                plan.folders.insert(dir.clone(), one.id.clone());
            }
            Some([one]) => plan.conflicts.push(format!(
                "{dir}: is a local directory but a remote {} ({})",
                one.mime_type, one.id
            )),
            Some(many) => plan.conflicts.push(clash(dir, many)),
        }
    }

    for (path, file) in &local.files {
        match remote.items.get(path).map(Vec::as_slice) {
            None | Some([]) => plan.upload.push(path.clone()),
            Some([one]) if one.is_folder() => plan.conflicts.push(format!(
                "{path}: is a local file but a remote folder ({})",
                one.id
            )),
            Some([one]) if one.is_google_native() => plan.conflicts.push(format!(
                "{path}: is a local file but a remote {} ({}), which has no binary content to compare",
                one.mime_type, one.id
            )),
            Some([one]) => {
                let same = one.size_bytes() == Some(file.size)
                    && match one.md5_checksum.as_deref() {
                        Some(remote_md5) => md5_of(&file.abs)? == remote_md5,
                        None => false,
                    };
                if same {
                    plan.unchanged.insert(path.clone(), one.id.clone());
                } else {
                    plan.update.insert(path.clone(), one.id.clone());
                }
            }
            Some(many) => plan.conflicts.push(clash(path, many)),
        }
    }

    if delete {
        for (path, items) in &remote.items {
            if local.files.contains_key(path) || local.dirs.contains(path) {
                continue;
            }
            // walk_remote only lists children of matched folders, so the
            // parent of every remaining path exists locally.
            debug_assert!({
                let p = parent_of(path);
                p.is_empty() || local.dirs.contains(p)
            });
            for (n, item) in items.iter().enumerate() {
                let key = if items.len() > 1 {
                    format!("{path} [{}]", n + 1)
                } else {
                    path.clone()
                };
                if item.is_google_native() && !item.is_folder() {
                    plan.kept.insert(key, item.id.clone());
                } else {
                    plan.trash.insert(key, item.id.clone());
                }
            }
        }
    }
    Ok(plan)
}

/// The record written by `--ids`, in the shape of the hand-kept drive.json
/// files it replaces.
#[derive(Debug, Default, Serialize)]
pub(crate) struct IdMap {
    pub root: String,
    pub folders: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Summary {
    pub folder: String,
    pub path: String,
    pub dry_run: bool,
    pub new: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: usize,
    pub trashed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub folders_created: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept_google_files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped_local: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids_file: Option<String>,
}

impl Summary {
    pub fn line(&self) -> String {
        format!(
            "{}{} new, {} updated, {} unchanged, {} trashed{}{}",
            if self.dry_run { "dry run: " } else { "" },
            self.new.len(),
            self.updated.len(),
            self.unchanged,
            self.trashed.len(),
            if self.folders_created.is_empty() {
                String::new()
            } else {
                format!(", {} folders created", self.folders_created.len())
            },
            if self.errors.is_empty() {
                String::new()
            } else {
                format!(", {} errors", self.errors.len())
            }
        )
    }
}

enum Job {
    New(String),
    Update(String, String),
}

/// Uploads one new file or updates one changed file.
async fn run_job(
    client: &DriveClient,
    root_id: &str,
    folders: &BTreeMap<String, String>,
    files: &BTreeMap<String, LocalFile>,
    job: Job,
) -> (bool, String, Result<DriveFile, GwsError>) {
    let (is_new, path) = match &job {
        Job::New(p) => (true, p.clone()),
        Job::Update(p, _) => (false, p.clone()),
    };
    let local_file = &files[&path];
    let media = Media::File {
        path: &local_file.abs,
        size: local_file.size,
    };
    let mime = mime_guess2::from_path(&local_file.abs)
        .first_or_octet_stream()
        .to_string();
    let result = match &job {
        Job::New(p) => {
            let parent = parent_of(p);
            let parent_id = if parent.is_empty() {
                Some(root_id.to_string())
            } else {
                folders.get(parent).cloned()
            };
            match parent_id {
                Some(parent_id) => {
                    let name = p.rsplit('/').next().unwrap_or(p);
                    client
                        .create_with_media(
                            &json!({ "name": name, "parents": [parent_id] }),
                            media,
                            &mime,
                        )
                        .await
                }
                None => Err(GwsError::Validation(
                    "parent folder was not created".to_string(),
                )),
            }
        }
        Job::Update(_, id) => client.update_media(id, media, &mime).await,
    };
    (is_new, path, result)
}

pub(crate) struct SyncOptions<'a> {
    pub local: &'a Path,
    pub target: &'a str,
    pub dry_run: bool,
    pub delete: bool,
    pub excludes: Vec<String>,
    pub ids_path: Option<PathBuf>,
}

pub(crate) async fn run_sync(
    client: &DriveClient,
    opts: SyncOptions<'_>,
) -> Result<Summary, GwsError> {
    let local = walk_local(opts.local, &opts.excludes)?;
    let create = if opts.dry_run {
        Create::DryRun
    } else {
        Create::Yes
    };
    let root = resolve_target(client, opts.target, Expect::Folder, create).await?;
    let root_id = root.file.id.clone();
    let drive_id = root.file.drive_id.clone();

    let remote = if root_id.is_empty() {
        RemoteTree::default()
    } else {
        walk_remote(client, &root_id, drive_id.as_deref(), &local.dirs).await?
    };
    let plan = plan(&local, &remote, opts.delete, md5_file)?;
    if !plan.conflicts.is_empty() {
        let mut msg = format!(
            "Nothing was changed: {} path(s) need a decision first. Rename or remove one side, or sync into another folder:",
            plan.conflicts.len()
        );
        for c in &plan.conflicts {
            msg.push_str("\n  ");
            msg.push_str(&sanitize_for_terminal(c));
        }
        return Err(GwsError::Validation(msg));
    }

    let mut summary = Summary {
        folder: root_id.clone(),
        path: root.path.clone(),
        dry_run: opts.dry_run,
        unchanged: plan.unchanged.len(),
        skipped_local: local.skipped.clone(),
        kept_google_files: plan.kept.keys().cloned().collect(),
        ..Default::default()
    };
    summary.folders_created.extend(root.created.iter().cloned());

    if opts.dry_run {
        summary
            .folders_created
            .extend(root.would_create.iter().cloned());
        summary
            .folders_created
            .extend(plan.create_folders.iter().cloned());
        summary.new = plan.upload.clone();
        summary.updated = plan.update.keys().cloned().collect();
        summary.trashed = plan.trash.keys().cloned().collect();
        return Ok(summary);
    }

    let mut ids = IdMap {
        root: root_id.clone(),
        folders: plan.folders.clone(),
        files: plan.unchanged.clone(),
    };

    // Folders first, parents before children (BTreeSet order guarantees it).
    for dir in &plan.create_folders {
        let parent = parent_of(dir);
        let parent_id = if parent.is_empty() {
            root_id.clone()
        } else {
            match ids.folders.get(parent) {
                Some(id) => id.clone(),
                None => {
                    summary
                        .errors
                        .push(format!("{dir}: parent folder was not created"));
                    continue;
                }
            }
        };
        let name = dir.rsplit('/').next().unwrap_or(dir);
        match client.create_folder(name, &parent_id).await {
            Ok(f) => {
                eprintln!("  folder   {}", sanitize_for_terminal(dir));
                ids.folders.insert(dir.clone(), f.id);
                summary.folders_created.push(dir.clone());
            }
            Err(e) => summary.errors.push(format!("{dir}: {e}")),
        }
    }

    let jobs: Vec<Job> = plan
        .upload
        .iter()
        .map(|p| Job::New(p.clone()))
        .chain(
            plan.update
                .iter()
                .map(|(p, id)| Job::Update(p.clone(), id.clone())),
        )
        .collect();
    let mut futures = Vec::with_capacity(jobs.len());
    for job in jobs {
        futures.push(run_job(client, &root_id, &ids.folders, &local.files, job));
    }
    let results: Vec<(bool, String, Result<DriveFile, GwsError>)> =
        futures_util::stream::iter(futures)
            .buffer_unordered(CONCURRENCY)
            .collect()
            .await;

    let mut new_files = BTreeMap::new();
    for (is_new, path, result) in results {
        match result {
            Ok(f) => {
                eprintln!(
                    "  {}  {}",
                    if is_new { "new    " } else { "updated" },
                    sanitize_for_terminal(&path)
                );
                if is_new {
                    summary.new.push(path.clone());
                } else {
                    summary.updated.push(path.clone());
                }
                new_files.insert(path, f.id);
            }
            Err(e) => summary.errors.push(format!("{path}: {e}")),
        }
    }
    ids.files.extend(new_files);
    summary.new.sort();
    summary.updated.sort();

    for (path, id) in &plan.trash {
        match client.trash(id).await {
            Ok(_) => {
                eprintln!("  trashed  {}", sanitize_for_terminal(path));
                summary.trashed.push(path.clone());
            }
            Err(e) => summary.errors.push(format!("{path}: {e}")),
        }
    }

    if let Some(ids_path) = &opts.ids_path {
        let text = serde_json::to_string_pretty(&ids)
            .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to encode ids: {e}")))?;
        std::fs::write(ids_path, text + "\n").map_err(|e| {
            GwsError::Validation(format!("Failed to write '{}': {e}", ids_path.display()))
        })?;
        summary.ids_file = Some(ids_path.display().to_string());
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::drive::api::FOLDER_MIME;

    fn remote_file(id: &str, name: &str, size: u64, md5: &str) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: name.into(),
            mime_type: "image/png".into(),
            size: Some(size.to_string()),
            md5_checksum: Some(md5.into()),
            ..Default::default()
        }
    }

    fn remote_folder(id: &str, name: &str) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: name.into(),
            mime_type: FOLDER_MIME.into(),
            ..Default::default()
        }
    }

    fn local(files: &[(&str, u64)], dirs: &[&str]) -> LocalTree {
        LocalTree {
            files: files
                .iter()
                .map(|(p, s)| {
                    (
                        p.to_string(),
                        LocalFile {
                            abs: PathBuf::from(p),
                            size: *s,
                        },
                    )
                })
                .collect(),
            dirs: dirs.iter().map(|d| d.to_string()).collect(),
            skipped: vec![],
        }
    }

    fn remote(items: Vec<(&str, DriveFile)>) -> RemoteTree {
        let mut t = RemoteTree::default();
        for (p, f) in items {
            t.items.entry(p.to_string()).or_default().push(f);
        }
        t
    }

    /// Local "md5" is the file name, so tests choose what matches.
    fn fake_md5(p: &Path) -> Result<String, GwsError> {
        Ok(format!("md5:{}", p.display()))
    }

    #[test]
    fn glob_matching() {
        assert!(glob_match("*.png", "a.png"));
        assert!(!glob_match("*.png", "a.pngx"));
        assert!(glob_match("draft-??.md", "draft-01.md"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
        assert!(is_excluded(
            "x/.DS_Store",
            ".DS_Store",
            &[".DS_Store".into()]
        ));
        assert!(is_excluded("out/tmp/a.png", "a.png", &["out/tmp/*".into()]));
        assert!(!is_excluded("out/a.png", "a.png", &["out/tmp/*".into()]));
    }

    #[test]
    fn plan_new_changed_unchanged() {
        let l = local(&[("a.png", 10), ("b.png", 20), ("c.png", 30)], &[]);
        let r = remote(vec![
            ("b.png", remote_file("B", "b.png", 20, "md5:b.png")),
            ("c.png", remote_file("C", "c.png", 30, "stale")),
        ]);
        let p = plan(&l, &r, false, fake_md5).unwrap();
        assert_eq!(p.upload, vec!["a.png"]);
        assert_eq!(p.unchanged.get("b.png").map(String::as_str), Some("B"));
        assert_eq!(p.update.get("c.png").map(String::as_str), Some("C"));
        assert!(p.trash.is_empty() && p.conflicts.is_empty());
    }

    #[test]
    fn size_mismatch_skips_hashing() {
        let l = local(&[("a.png", 11)], &[]);
        let r = remote(vec![("a.png", remote_file("A", "a.png", 10, "x"))]);
        let p = plan(&l, &r, false, |_| panic!("must not hash when sizes differ")).unwrap();
        assert_eq!(p.update.get("a.png").map(String::as_str), Some("A"));
    }

    #[test]
    fn plan_folders_and_nested_files() {
        let l = local(&[("d/x.png", 1), ("e/y.png", 2)], &["d", "e", "e/f"]);
        let r = remote(vec![("d", remote_folder("D", "d"))]);
        let p = plan(&l, &r, false, fake_md5).unwrap();
        assert_eq!(p.folders.get("d").map(String::as_str), Some("D"));
        assert_eq!(p.create_folders, vec!["e", "e/f"]);
        assert_eq!(p.upload, vec!["d/x.png", "e/y.png"]);
    }

    #[test]
    fn delete_trashes_extras_but_keeps_google_files() {
        let l = local(&[("a.png", 1)], &[]);
        let mut doc = remote_file("G", "notes", 0, "");
        doc.mime_type = "application/vnd.google-apps.document".into();
        doc.size = None;
        doc.md5_checksum = None;
        let r = remote(vec![
            ("a.png", remote_file("A", "a.png", 1, "md5:a.png")),
            ("old.png", remote_file("O", "old.png", 5, "x")),
            ("old-dir", remote_folder("F", "old-dir")),
            ("notes", doc),
        ]);
        let p = plan(&l, &r, true, fake_md5).unwrap();
        assert_eq!(
            p.trash.keys().collect::<Vec<_>>(),
            vec!["old-dir", "old.png"]
        );
        assert_eq!(p.kept.get("notes").map(String::as_str), Some("G"));

        let without = plan(&l, &r, false, fake_md5).unwrap();
        assert!(without.trash.is_empty() && without.kept.is_empty());
    }

    #[test]
    fn duplicates_not_present_locally_are_all_trashed() {
        let l = local(&[], &[]);
        let r = remote(vec![
            ("dup.png", remote_file("1", "dup.png", 1, "x")),
            ("dup.png", remote_file("2", "dup.png", 1, "x")),
        ]);
        let p = plan(&l, &r, true, fake_md5).unwrap();
        assert_eq!(p.trash.len(), 2);
        assert!(p.conflicts.is_empty());
    }

    #[test]
    fn conflicts_are_reported_not_guessed() {
        let l = local(&[("dup.png", 1), ("x", 1), ("doc", 1)], &["f"]);
        let mut gdoc = remote_file("G", "doc", 0, "");
        gdoc.mime_type = "application/vnd.google-apps.document".into();
        gdoc.md5_checksum = None;
        let r = remote(vec![
            ("dup.png", remote_file("1", "dup.png", 1, "md5:dup.png")),
            ("dup.png", remote_file("2", "dup.png", 1, "md5:dup.png")),
            ("x", remote_folder("X", "x")),
            ("f", remote_file("F", "f", 1, "y")),
            ("doc", gdoc),
        ]);
        let p = plan(&l, &r, true, fake_md5).unwrap();
        assert_eq!(p.conflicts.len(), 4, "{:?}", p.conflicts);
        assert!(p
            .conflicts
            .iter()
            .any(|c| c.starts_with("dup.png: 2 remote items")));
        assert!(p
            .conflicts
            .iter()
            .any(|c| c.starts_with("x: is a local file")));
        assert!(p
            .conflicts
            .iter()
            .any(|c| c.starts_with("f: is a local directory")));
        assert!(p
            .conflicts
            .iter()
            .any(|c| c.starts_with("doc: is a local file")));
    }

    #[test]
    fn walk_local_and_md5() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub/deeper")).unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("sub/deeper/b.txt"), b"").unwrap();
        std::fs::write(dir.path().join(".DS_Store"), b"junk").unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), b"ref").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", dir.path().join("link")).unwrap();

        let excludes: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect();
        let tree = walk_local(dir.path(), &excludes).unwrap();
        assert_eq!(
            tree.files.keys().collect::<Vec<_>>(),
            vec!["a.txt", "sub/deeper/b.txt"]
        );
        assert_eq!(
            tree.dirs.iter().collect::<Vec<_>>(),
            vec!["sub", "sub/deeper"]
        );
        #[cfg(unix)]
        assert_eq!(tree.skipped, vec!["link"]);
        assert_eq!(tree.files["a.txt"].size, 5);

        assert_eq!(
            md5_file(&dir.path().join("a.txt")).unwrap(),
            "5d41402abc4b2a76b9719d911017c592"
        );
        assert_eq!(
            md5_file(&dir.path().join("sub/deeper/b.txt")).unwrap(),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
    }

    #[test]
    fn summary_line() {
        let s = Summary {
            new: vec!["a".into()],
            updated: vec!["b".into(), "c".into()],
            unchanged: 4,
            ..Default::default()
        };
        assert_eq!(s.line(), "1 new, 2 updated, 4 unchanged, 0 trashed");
        let d = Summary {
            dry_run: true,
            folders_created: vec!["x".into()],
            ..Default::default()
        };
        assert_eq!(
            d.line(),
            "dry run: 0 new, 0 updated, 0 unchanged, 0 trashed, 1 folders created"
        );
    }
}

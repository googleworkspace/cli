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

//! `gws meet +recording`: recordings, their Drive files, and downloads.

use super::api::{check_status, list_all, meet_url, HttpApi, MeetApi, DRIVE_BASE};
use super::records::{
    format_duration, not_found, resolve_record, span_seconds, str_field, RecordRef,
};
use crate::error::GwsError;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub(super) async fn recordings(api: &dyn MeetApi, r: &RecordRef) -> Result<Value, GwsError> {
    let record = resolve_record(api, r).await?;
    let name = str_field(&record, "name").unwrap_or_default().to_string();
    let recs = list_all(
        api,
        &meet_url(&format!("{name}/recordings")),
        "recordings",
        &vec![],
        None,
    )
    .await?;
    if recs.is_empty() {
        return Err(not_found(&format!(
            "No recordings for {name}. Meet only keeps one when someone recorded the call."
        )));
    }

    let now = chrono::Utc::now();
    let mut rows = Vec::new();
    for rec in &recs {
        let file = rec
            .get("driveDestination")
            .and_then(|d| str_field(d, "file"))
            .map(str::to_string);
        let meta = match &file {
            Some(id) => api
                .get_json(
                    &format!(
                        "{DRIVE_BASE}/files/{}",
                        crate::validate::encode_path_segment(id)
                    ),
                    &vec![
                        ("fields", "name,size,mimeType".to_string()),
                        ("supportsAllDrives", "true".to_string()),
                    ],
                )
                .await
                .unwrap_or(Value::Null),
            None => Value::Null,
        };
        let secs = span_seconds(str_field(rec, "startTime"), str_field(rec, "endTime"), now);
        rows.push(json!({
            "recording": str_field(rec, "name"),
            "state": str_field(rec, "state"),
            "start": str_field(rec, "startTime"),
            "duration": format_duration(secs),
            "file": file,
            "fileName": str_field(&meta, "name"),
            "mimeType": str_field(&meta, "mimeType"),
            "size": meta.get("size").and_then(|s| s.as_str().and_then(|s| s.parse::<u64>().ok()).or_else(|| s.as_u64())),
            "url": rec.get("driveDestination").and_then(|d| str_field(d, "exportUri")),
        }));
    }

    Ok(json!({
        "record": name,
        "count": rows.len(),
        "recordings": rows,
    }))
}

/// A file name safe to join onto the output directory: no separators,
/// no control characters, no leading dots, at most 200 characters.
pub(super) fn safe_file_name(name: Option<&str>, fallback: &str) -> String {
    let cleaned: String = name
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c == '/'
                || c == '\\'
                || c == ':'
                || c.is_control()
                || crate::validate::is_dangerous_unicode(c)
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').trim();
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    trimmed.chars().take(200).collect()
}

/// File extension for a recording's media type. Drive names recordings like
/// "Meeting (2026-01-01 10:00 GMT) – Recording", with no extension.
pub(super) fn media_extension(mime: &str) -> Option<&'static str> {
    match mime.split(';').next().unwrap_or("").trim() {
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "video/quicktime" => Some("mov"),
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => Some("m4a"),
        "audio/mpeg" => Some("mp3"),
        _ => None,
    }
}

/// What to do with one recording when downloading.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Plan {
    /// Not generated yet, or no Drive file.
    Skip(&'static str),
    /// The same file is already on disk at full size.
    AlreadyThere(PathBuf),
    Download {
        file: String,
        target: PathBuf,
    },
}

pub(super) fn plan_download(row: &Value, dir: &Path) -> Plan {
    let Some(file) = str_field(row, "file") else {
        return Plan::Skip("no Drive file");
    };
    if str_field(row, "state") != Some("FILE_GENERATED") {
        return Plan::Skip("file not generated yet");
    }
    let id_tail = str_field(row, "recording")
        .and_then(|r| r.rsplit('/').next())
        .unwrap_or("recording");
    let fallback = format!("meet-recording-{}.mp4", safe_file_name(Some(id_tail), "x"));
    let mut file_name = safe_file_name(str_field(row, "fileName"), &fallback);
    if let Some(ext) = str_field(row, "mimeType").and_then(media_extension) {
        let has_ext = Path::new(&file_name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(ext));
        if !has_ext {
            file_name = format!("{file_name}.{ext}");
        }
    }
    let target = dir.join(file_name);
    let size = row.get("size").and_then(Value::as_u64);
    if let (Some(size), Ok(meta)) = (size, std::fs::metadata(&target)) {
        if meta.len() == size {
            return Plan::AlreadyThere(target);
        }
    }
    Plan::Download {
        file: file.to_string(),
        target,
    }
}

/// The partial file a download streams into before it is renamed.
pub(super) fn part_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!("{name}.part"))
}

/// Whether the response continues an existing `.part` file. Only a
/// `206 Partial Content` reply to our Range request may be appended; a
/// plain `200` is the whole file and starts over.
pub(super) fn resumes(status: u16, existing: u64) -> bool {
    existing > 0 && status == 206
}

/// Stream a Drive file to `target` through a `.part` file, then rename.
/// An interrupted download resumes from its `.part` file on the next run.
async fn download_file(api: &HttpApi, file: &str, target: &Path) -> Result<u64, GwsError> {
    let url = format!(
        "{DRIVE_BASE}/files/{}",
        crate::validate::encode_path_segment(file)
    );
    let part = part_path(target);
    let io_err = |e: std::io::Error| GwsError::Other(anyhow::anyhow!("{}: {e}", part.display()));
    let existing = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let resp = crate::client::send_with_retry(|| {
        let req = api
            .client
            .get(&url)
            .query(&[("alt", "media"), ("supportsAllDrives", "true")])
            .bearer_auth(&api.token);
        if existing > 0 {
            req.header(reqwest::header::RANGE, format!("bytes={existing}-"))
        } else {
            req
        }
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Download failed: {e}")))?;

    // 416: the .part file already holds every byte.
    if existing > 0 && resp.status().as_u16() == 416 {
        tokio::fs::rename(&part, target).await.map_err(io_err)?;
        return Ok(existing);
    }
    let resp = check_status(resp).await?;
    let append = resumes(resp.status().as_u16(), existing);
    if append {
        crate::output::info(&format!("Resuming at {existing} bytes"));
    }
    let mut out = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&part)
        .await
        .map_err(io_err)?;

    let mut stream = resp.bytes_stream();
    let mut total = if append { existing } else { 0 };
    let mut next_report = total + PROGRESS_STEP;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            GwsError::Other(anyhow::anyhow!(
                "Download interrupted at {total} bytes ({e}). Run the same command again to resume."
            ))
        })?;
        out.write_all(&chunk).await.map_err(io_err)?;
        total += chunk.len() as u64;
        if total >= next_report {
            crate::output::info(&format!("  {} MB", total / 1_000_000));
            next_report += PROGRESS_STEP;
        }
    }
    out.flush().await.map_err(io_err)?;
    drop(out);
    tokio::fs::rename(&part, target).await.map_err(io_err)?;
    Ok(total)
}

const PROGRESS_STEP: u64 = 50_000_000;

/// Download every generated recording into `dir`, annotating each row with
/// `savedTo` (or `skipped`).
pub(super) async fn download_all(
    api: &HttpApi,
    out: &mut Value,
    dir: &Path,
) -> Result<(), GwsError> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("{}: {e}", dir.display())))?;
    let mut saved = 0;
    if let Some(rows) = out["recordings"].as_array_mut() {
        for row in rows.iter_mut() {
            match plan_download(row, dir) {
                Plan::Skip(why) => row["skipped"] = json!(why),
                Plan::AlreadyThere(p) => {
                    row["savedTo"] = json!(p.display().to_string());
                    row["skipped"] = json!("already downloaded");
                    saved += 1;
                }
                Plan::Download { file, target } => {
                    crate::output::info(&format!("Downloading {} ...", target.display()));
                    let bytes = download_file(api, &file, &target).await?;
                    row["savedTo"] = json!(target.display().to_string());
                    row["bytes"] = json!(bytes);
                    saved += 1;
                }
            }
        }
    }
    if saved == 0 {
        return Err(not_found(
            "No recording could be downloaded: none has a generated Drive file yet.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::api::fake::FakeApi;
    use super::super::api::MEET_BASE;
    use super::*;

    fn rec() -> RecordRef {
        RecordRef::Name("conferenceRecords/r".into())
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(
            safe_file_name(Some("Call (2026).mp4"), "f"),
            "Call (2026).mp4"
        );
        assert_eq!(safe_file_name(Some("../../.ssh/id"), "f"), "_.._.ssh_id");
        assert_eq!(safe_file_name(Some("..."), "f"), "f");
        assert_eq!(safe_file_name(Some(".hidden"), "f"), "hidden");
        assert_eq!(safe_file_name(Some("a\nb\\c:d"), "f"), "a_b_c_d");
        assert_eq!(safe_file_name(None, "f"), "f");
        assert_eq!(safe_file_name(Some(&"x".repeat(300)), "f").len(), 200);
    }

    #[test]
    fn resume_rules() {
        assert_eq!(
            part_path(Path::new("d/a.mp4")),
            PathBuf::from("d/a.mp4.part")
        );
        assert!(resumes(206, 10));
        assert!(!resumes(200, 10));
        assert!(!resumes(206, 0));
    }

    #[test]
    fn media_extensions() {
        assert_eq!(media_extension("video/mp4"), Some("mp4"));
        assert_eq!(media_extension("video/webm; codecs=vp9"), Some("webm"));
        assert_eq!(media_extension("audio/x-m4a"), Some("m4a"));
        assert_eq!(media_extension("application/octet-stream"), None);
    }

    #[test]
    fn appends_missing_extension() {
        let dir = tempfile::tempdir().unwrap();
        let row = json!({"state": "FILE_GENERATED", "file": "f", "mimeType": "video/mp4",
                         "fileName": "Sync (2026-01-01) – Recording"});
        assert_eq!(
            plan_download(&row, dir.path()),
            Plan::Download {
                file: "f".into(),
                target: dir.path().join("Sync (2026-01-01) – Recording.mp4")
            }
        );
        let has = json!({"state": "FILE_GENERATED", "file": "f", "mimeType": "video/mp4", "fileName": "a.MP4"});
        assert_eq!(
            plan_download(&has, dir.path()),
            Plan::Download {
                file: "f".into(),
                target: dir.path().join("a.MP4")
            }
        );
    }

    #[test]
    fn plans_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let row = json!({"recording": "conferenceRecords/r/recordings/r1", "state": "FILE_GENERATED",
                         "file": "f1", "fileName": "Call.mp4", "size": 3});
        assert_eq!(
            plan_download(&row, dir.path()),
            Plan::Download {
                file: "f1".into(),
                target: dir.path().join("Call.mp4")
            }
        );
        std::fs::write(dir.path().join("Call.mp4"), b"abc").unwrap();
        assert_eq!(
            plan_download(&row, dir.path()),
            Plan::AlreadyThere(dir.path().join("Call.mp4"))
        );
        // A partial file of the wrong size is downloaded again.
        std::fs::write(dir.path().join("Call.mp4"), b"ab").unwrap();
        assert!(matches!(
            plan_download(&row, dir.path()),
            Plan::Download { .. }
        ));

        let no_name = json!({"recording": "conferenceRecords/r/recordings/r1", "state": "FILE_GENERATED", "file": "f1"});
        assert_eq!(
            plan_download(&no_name, dir.path()),
            Plan::Download {
                file: "f1".into(),
                target: dir.path().join("meet-recording-r1.mp4")
            }
        );
        assert_eq!(
            plan_download(&json!({"state": "STARTED", "file": "f"}), dir.path()),
            Plan::Skip("file not generated yet")
        );
        assert_eq!(
            plan_download(&json!({}), dir.path()),
            Plan::Skip("no Drive file")
        );
    }

    #[tokio::test]
    async fn lists_recordings_with_drive_metadata() {
        let api = FakeApi::default()
            .with(&format!("{MEET_BASE}/conferenceRecords/r"), json!({"name": "conferenceRecords/r"}))
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/recordings"),
                json!({"recordings": [
                    {"name": "conferenceRecords/r/recordings/a", "state": "FILE_GENERATED",
                     "startTime": "2026-01-01T10:00:00Z", "endTime": "2026-01-01T10:30:00Z",
                     "driveDestination": {"file": "f1", "exportUri": "https://drive.google.com/f1"}},
                    {"name": "conferenceRecords/r/recordings/b", "state": "STARTED"}
                ]}),
            )
            .with(
                &format!("{DRIVE_BASE}/files/f1"),
                json!({"name": "Call.mp4", "size": "1024", "mimeType": "video/mp4"}),
            );
        let out = recordings(&api, &rec()).await.unwrap();
        assert_eq!(out["count"], 2);
        let r = &out["recordings"];
        assert_eq!(r[0]["fileName"], "Call.mp4");
        assert_eq!(r[0]["size"], 1024);
        assert_eq!(r[0]["duration"], "30m 00s");
        assert_eq!(r[1]["file"], Value::Null);
        let q = &api.queries_for(&format!("{DRIVE_BASE}/files/f1"))[0];
        assert!(q.contains(&("supportsAllDrives", "true".to_string())));
    }

    #[tokio::test]
    async fn no_recordings_is_an_error() {
        let api = FakeApi::default()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r"),
                json!({"name": "conferenceRecords/r"}),
            )
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/recordings"),
                json!({}),
            );
        assert!(recordings(&api, &rec())
            .await
            .unwrap_err()
            .to_string()
            .contains("No recordings"));
    }

    #[tokio::test]
    async fn download_all_with_nothing_ready_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let api = HttpApi::new("t".into()).unwrap();
        let mut out = json!({"recordings": [{"state": "STARTED", "file": "f"}]});
        let err = download_all(&api, &mut out, &dir.path().join("sub"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No recording could be downloaded"));
        assert_eq!(out["recordings"][0]["skipped"], "file not generated yet");
        assert!(dir.path().join("sub").is_dir());
    }

    #[tokio::test]
    async fn download_all_counts_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Call.mp4"), b"abc").unwrap();
        let api = HttpApi::new("t".into()).unwrap();
        let mut out = json!({"recordings": [{"state": "FILE_GENERATED", "file": "f",
            "fileName": "Call.mp4", "size": 3}]});
        download_all(&api, &mut out, dir.path()).await.unwrap();
        assert_eq!(out["recordings"][0]["skipped"], "already downloaded");
    }
}

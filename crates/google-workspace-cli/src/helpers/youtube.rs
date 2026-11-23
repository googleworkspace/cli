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

//! YouTube helpers:
//! - `gws youtube +upload <video>`: a resumable upload with progress, then an
//!   optional custom thumbnail and playlist entry.
//! - `gws youtube +videos`: the channel's most recent uploads with their stats.

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::formatter::OutputFormat;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, Instant};

const API_BASE: &str = "https://www.googleapis.com/youtube/v3";
const UPLOAD_BASE: &str = "https://www.googleapis.com/upload/youtube/v3";
const SCOPE_UPLOAD: &str = "https://www.googleapis.com/auth/youtube.upload";
const SCOPE_READ: &str = "https://www.googleapis.com/auth/youtube.readonly";

/// Resumable chunks must be multiples of 256 KiB; 8 MiB balances progress
/// granularity against request overhead.
const CHUNK_SIZE: u64 = 8 * 1024 * 1024;
const MAX_RETRIES: u32 = 8;
const MAX_TITLE_CHARS: usize = 100;
const MAX_DESCRIPTION_BYTES: usize = 5000;
const MAX_TAGS_CHARS: usize = 500;
const MAX_THUMBNAIL_BYTES: u64 = 2 * 1024 * 1024;
const PRIVACY: &[&str] = &["private", "unlisted", "public"];

pub struct YouTubeHelper;

impl Helper for YouTubeHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+upload")
                .about("[Helper] Upload a video (resumable, with progress); private by default")
                .arg(
                    Arg::new("video")
                        .help("Path to the video file (under the current directory)")
                        .required(true)
                        .value_name("VIDEO"),
                )
                .arg(
                    Arg::new("title")
                        .long("title")
                        .help("Video title (max 100 characters)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("description")
                        .long("description")
                        .help("Description text, or @path to read it from a file")
                        .value_name("TEXT|@FILE"),
                )
                .arg(
                    Arg::new("tags")
                        .long("tags")
                        .help("Comma-separated tags")
                        .value_name("A,B"),
                )
                .arg(
                    Arg::new("privacy")
                        .long("privacy")
                        .help("private (default), unlisted or public")
                        .value_parser(clap::builder::PossibleValuesParser::new(PRIVACY))
                        .default_value("private")
                        .value_name("STATUS"),
                )
                .arg(
                    Arg::new("publish-at")
                        .long("publish-at")
                        .help("Schedule publication (RFC 3339, e.g. 2026-12-01T09:00:00+01:00); keeps the video private until then")
                        .value_name("TIME"),
                )
                .arg(
                    Arg::new("category")
                        .long("category")
                        .help("YouTube category ID (default 22, People & Blogs; 27 is Education)")
                        .default_value("22")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("thumbnail")
                        .long("thumbnail")
                        .help("Custom thumbnail image (JPEG or PNG, max 2 MB)")
                        .value_name("IMAGE"),
                )
                .arg(
                    Arg::new("playlist")
                        .long("playlist")
                        .help("Add the video to this playlist ID (needs the youtube scope)")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("made-for-kids")
                        .long("made-for-kids")
                        .help("Declare the video as made for kids (default: not made for kids)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("no-notify")
                        .long("no-notify")
                        .help("Don't notify subscribers about a public upload")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws youtube +upload film.mp4 --title 'MBPrep in 60 seconds'
  gws youtube +upload film.mp4 --title 'Launch' --description @description.txt --tags mbprep,medicine
  gws youtube +upload film.mp4 --title 'Launch' --privacy unlisted --thumbnail cover.jpg --playlist PLxxxx
  gws youtube +upload film.mp4 --title 'Launch' --dry-run

TIPS:
  Uploads are private unless you pass --privacy.
  The upload resumes after network errors; progress is shown on stderr.
  --dry-run prints the requests without reading credentials or uploading.
  Custom thumbnails need a verified channel. --playlist needs the youtube
  (or youtube.force-ssl) scope; youtube.upload alone can't add to playlists.",
                ),
        )
        .subcommand(
            Command::new("+videos")
                .about("[Helper] List the channel's most recent uploads with views and status")
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("How many videos (default 10, max 500)")
                        .value_parser(clap::value_parser!(u32).range(1..=500))
                        .default_value("10")
                        .value_name("N"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws youtube +videos
  gws youtube +videos --limit 50 --format csv

TIPS:
  Read-only. Includes private and unlisted uploads of the signed-in channel.",
                ),
        )
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(m) = matches.subcommand_matches("+upload") {
                handle_upload(m).await?;
                return Ok(true);
            }
            if let Some(m) = matches.subcommand_matches("+videos") {
                handle_videos(m).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

// ── +upload: options ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UploadOptions {
    pub video: PathBuf,
    pub video_size: u64,
    pub content_type: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub privacy: String,
    pub publish_at: Option<String>,
    pub category: String,
    pub thumbnail: Option<PathBuf>,
    pub playlist: Option<String>,
    pub made_for_kids: bool,
    pub notify: bool,
}

pub(crate) fn validate_title(title: &str) -> Result<String, GwsError> {
    crate::validate::reject_dangerous_chars(title, "--title")?;
    let t = title.trim();
    if t.is_empty() {
        return Err(GwsError::Validation("--title must not be empty".into()));
    }
    if t.chars().count() > MAX_TITLE_CHARS {
        return Err(GwsError::Validation(format!(
            "--title is {} characters; YouTube allows at most {MAX_TITLE_CHARS}",
            t.chars().count()
        )));
    }
    if t.contains('<') || t.contains('>') {
        return Err(GwsError::Validation(
            "--title cannot contain '<' or '>' (YouTube rejects them)".into(),
        ));
    }
    Ok(t.to_string())
}

/// Checks description text: YouTube's limit is 5000 bytes and no angle brackets.
pub(crate) fn validate_description(text: &str) -> Result<String, GwsError> {
    if text.len() > MAX_DESCRIPTION_BYTES {
        return Err(GwsError::Validation(format!(
            "Description is {} bytes; YouTube allows at most {MAX_DESCRIPTION_BYTES}",
            text.len()
        )));
    }
    if text.contains('<') || text.contains('>') {
        return Err(GwsError::Validation(
            "Description cannot contain '<' or '>' (YouTube rejects them)".into(),
        ));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        return Err(GwsError::Validation(
            "Description contains control characters".into(),
        ));
    }
    Ok(text.to_string())
}

/// Resolves `--description`: literal text, or `@path` read from a file
/// under the current directory.
pub(crate) fn read_description(arg: Option<&str>) -> Result<String, GwsError> {
    let Some(arg) = arg else {
        return Ok(String::new());
    };
    if let Some(path) = arg.strip_prefix('@') {
        let p = crate::validate::validate_safe_file_path(path, "--description")?;
        let text = std::fs::read_to_string(&p).map_err(|e| {
            GwsError::Validation(format!("Cannot read description file '{path}': {e}"))
        })?;
        return validate_description(text.trim_end());
    }
    validate_description(arg)
}

pub(crate) fn parse_tags(raw: Option<&str>) -> Result<Vec<String>, GwsError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    crate::validate::reject_dangerous_chars(raw, "--tags")?;
    let mut tags: Vec<String> = Vec::new();
    for t in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if t.contains('<') || t.contains('>') {
            return Err(GwsError::Validation(format!(
                "Tag '{t}' cannot contain '<' or '>'"
            )));
        }
        if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            tags.push(t.to_string());
        }
    }
    // YouTube counts a tag with a space as if it were quoted (+2 chars) and
    // a comma between tags.
    let total: usize = tags
        .iter()
        .map(|t| t.chars().count() + if t.contains(' ') { 2 } else { 0 })
        .sum::<usize>()
        + tags.len().saturating_sub(1);
    if total > MAX_TAGS_CHARS {
        return Err(GwsError::Validation(format!(
            "Tags total {total} characters; YouTube allows at most {MAX_TAGS_CHARS}"
        )));
    }
    Ok(tags)
}

/// YouTube IDs (playlists, categories) are short URL-safe tokens.
pub(crate) fn validate_youtube_id(id: &str, flag: &str) -> Result<String, GwsError> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(GwsError::Validation(format!(
            "{flag} '{id}' is not a valid YouTube ID"
        )));
    }
    Ok(id.to_string())
}

pub(crate) fn validate_publish_at(
    raw: &str,
    privacy: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String, GwsError> {
    let t = chrono::DateTime::parse_from_rfc3339(raw.trim()).map_err(|_| {
        GwsError::Validation(format!(
            "--publish-at must be RFC 3339, e.g. 2026-12-01T09:00:00+01:00; got '{raw}'"
        ))
    })?;
    if t.with_timezone(&chrono::Utc) <= now {
        return Err(GwsError::Validation(
            "--publish-at must be in the future".into(),
        ));
    }
    if privacy != "private" {
        return Err(GwsError::Validation(
            "--publish-at needs --privacy private: YouTube publishes a scheduled video itself"
                .into(),
        ));
    }
    Ok(t.with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// MIME type for a video file by extension (YouTube sniffs the content too).
pub(crate) fn video_content_type(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" => "video/mp4",
        "m4v" => "video/x-m4v",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "wmv" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "mpg" | "mpeg" => "video/mpeg",
        "3gp" => "video/3gpp",
        _ => "application/octet-stream",
    }
    .to_string()
}

pub(crate) fn thumbnail_content_type(path: &Path) -> Result<&'static str, GwsError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "png" => Ok("image/png"),
        _ => Err(GwsError::Validation(format!(
            "--thumbnail '{}' must be a .jpg, .jpeg or .png file",
            path.display()
        ))),
    }
}

fn parse_upload_options(
    matches: &ArgMatches,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<UploadOptions, GwsError> {
    let video_arg = matches.get_one::<String>("video").expect("required");
    let video = crate::validate::validate_safe_file_path(video_arg, "video")?;
    let meta = std::fs::metadata(&video)
        .map_err(|e| GwsError::Validation(format!("Cannot read video '{video_arg}': {e}")))?;
    if !meta.is_file() {
        return Err(GwsError::Validation(format!("'{video_arg}' is not a file")));
    }
    if meta.len() == 0 {
        return Err(GwsError::Validation(format!("'{video_arg}' is empty")));
    }
    let privacy = matches
        .get_one::<String>("privacy")
        .cloned()
        .unwrap_or_else(|| "private".into());
    let publish_at = matches
        .get_one::<String>("publish-at")
        .map(|p| validate_publish_at(p, &privacy, now))
        .transpose()?;
    let thumbnail = match matches.get_one::<String>("thumbnail") {
        Some(t) => {
            let p = crate::validate::validate_safe_file_path(t, "--thumbnail")?;
            thumbnail_content_type(&p)?;
            let size = std::fs::metadata(&p)
                .map_err(|e| GwsError::Validation(format!("Cannot read thumbnail '{t}': {e}")))?
                .len();
            if size > MAX_THUMBNAIL_BYTES {
                return Err(GwsError::Validation(format!(
                    "--thumbnail is {size} bytes; YouTube allows at most 2 MB"
                )));
            }
            Some(p)
        }
        None => None,
    };
    Ok(UploadOptions {
        content_type: video_content_type(&video),
        video,
        video_size: meta.len(),
        title: validate_title(matches.get_one::<String>("title").expect("required"))?,
        description: read_description(
            matches.get_one::<String>("description").map(String::as_str),
        )?,
        tags: parse_tags(matches.get_one::<String>("tags").map(String::as_str))?,
        privacy,
        publish_at,
        category: validate_youtube_id(
            matches
                .get_one::<String>("category")
                .map(String::as_str)
                .unwrap_or("22"),
            "--category",
        )?,
        thumbnail,
        playlist: matches
            .get_one::<String>("playlist")
            .map(|p| validate_youtube_id(p, "--playlist"))
            .transpose()?,
        made_for_kids: matches.get_flag("made-for-kids"),
        notify: !matches.get_flag("no-notify"),
    })
}

/// The `videos.insert` resource body.
pub(crate) fn build_video_resource(o: &UploadOptions) -> Value {
    let mut snippet = json!({
        "title": o.title,
        "description": o.description,
        "categoryId": o.category,
    });
    if !o.tags.is_empty() {
        snippet["tags"] = json!(o.tags);
    }
    let mut status = json!({
        "privacyStatus": o.privacy,
        "selfDeclaredMadeForKids": o.made_for_kids,
    });
    if let Some(p) = &o.publish_at {
        status["publishAt"] = json!(p);
    }
    json!({"snippet": snippet, "status": status})
}

pub(crate) fn init_query(o: &UploadOptions) -> Vec<(&'static str, String)> {
    vec![
        ("uploadType", "resumable".to_string()),
        ("part", "snippet,status".to_string()),
        ("notifySubscribers", o.notify.to_string()),
    ]
}

/// `Content-Range` for the chunk `[start, end]` (inclusive) of `total` bytes.
pub(crate) fn content_range(start: u64, end_inclusive: u64, total: u64) -> String {
    format!("bytes {start}-{end_inclusive}/{total}")
}

/// Next byte to send, from a `308 Resume Incomplete` response's `Range`
/// header (`bytes=0-N` means N+1 bytes are stored; no header means none).
pub(crate) fn next_offset_from_range(range: Option<&str>) -> Result<u64, GwsError> {
    let Some(r) = range else {
        return Ok(0);
    };
    let end = r
        .trim()
        .strip_prefix("bytes=")
        .and_then(|s| s.split('-').nth(1))
        .and_then(|e| e.trim().parse::<u64>().ok())
        .ok_or_else(|| {
            GwsError::Other(anyhow::anyhow!(
                "Unexpected Range header from upload: '{r}'"
            ))
        })?;
    Ok(end + 1)
}

/// Exponential backoff capped at 32 s.
pub(crate) fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(2u64.saturating_pow(attempt.min(5)))
}

/// The pause before resuming; zero under test so retry paths run instantly.
fn retry_delay(attempt: u32) -> Duration {
    if cfg!(test) {
        Duration::ZERO
    } else {
        backoff(attempt)
    }
}

pub(crate) fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

pub(crate) fn progress_line(sent: u64, total: u64, elapsed: Duration) -> String {
    let pct = if total == 0 {
        100.0
    } else {
        sent as f64 * 100.0 / total as f64
    };
    let secs = elapsed.as_secs_f64();
    let rate = if secs > 0.0 { sent as f64 / secs } else { 0.0 };
    let eta = if rate > 0.0 && sent < total {
        let s = ((total - sent) as f64 / rate).round() as u64;
        format!(" · ~{}m{:02}s left", s / 60, s % 60)
    } else {
        String::new()
    };
    format!(
        "Uploading {pct:5.1}% ({} / {}) at {}/s{eta}",
        human_bytes(sent),
        human_bytes(total),
        human_bytes(rate as u64)
    )
}

/// What `--dry-run` prints: every request the upload would make.
pub(crate) fn dry_run_plan(o: &UploadOptions, upload_base: &str, api_base: &str) -> Value {
    let chunks = o.video_size.div_ceil(CHUNK_SIZE);
    let mut steps = vec![json!({
        "step": "start resumable upload session",
        "method": "POST",
        "url": format!("{upload_base}/videos"),
        "query": init_query(o).into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
        "headers": {
            "Content-Type": "application/json; charset=UTF-8",
            "X-Upload-Content-Type": o.content_type,
            "X-Upload-Content-Length": o.video_size.to_string(),
        },
        "body": build_video_resource(o),
    })];
    steps.push(json!({
        "step": "upload the file",
        "method": "PUT",
        "url": "<session URI from the Location header>",
        "file": o.video.display().to_string(),
        "bytes": o.video_size,
        "chunks": chunks,
        "chunk_size": CHUNK_SIZE,
        "first_content_range": content_range(0, o.video_size.min(CHUNK_SIZE) - 1, o.video_size),
    }));
    if let Some(t) = &o.thumbnail {
        steps.push(json!({
            "step": "set custom thumbnail",
            "method": "POST",
            "url": format!("{upload_base}/thumbnails/set"),
            "query": {"videoId": "<new video ID>", "uploadType": "media"},
            "file": t.display().to_string(),
        }));
    }
    if let Some(p) = &o.playlist {
        steps.push(json!({
            "step": "add to playlist",
            "method": "POST",
            "url": format!("{api_base}/playlistItems"),
            "query": {"part": "snippet"},
            "body": {"snippet": {"playlistId": p, "resourceId": {"kind": "youtube#video", "videoId": "<new video ID>"}}},
        }));
    }
    json!({"dry_run": true, "requests": steps})
}

// ── +upload: transport ───────────────────────────────────────────────

async fn read_chunk(
    file: &mut tokio::fs::File,
    offset: u64,
    len: u64,
) -> Result<Vec<u8>, GwsError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to seek video file: {e}")))?;
    let mut buf = vec![0u8; len as usize];
    file.read_exact(&mut buf)
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to read video file: {e}")))?;
    Ok(buf)
}

/// Starts a resumable session and returns its session URI.
pub(crate) async fn start_session(
    client: &reqwest::Client,
    token: &str,
    upload_base: &str,
    o: &UploadOptions,
) -> Result<String, GwsError> {
    let body = build_video_resource(o);
    let resp = crate::client::send_with_retry(|| {
        client
            .post(format!("{upload_base}/videos"))
            .query(&init_query(o))
            .bearer_auth(token)
            .header("X-Upload-Content-Type", &o.content_type)
            .header("X-Upload-Content-Length", o.video_size.to_string())
            .json(&body)
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to start YouTube upload: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(crate::executor::api_error(status, &text));
    }
    resp.headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| {
            GwsError::Other(anyhow::anyhow!(
                "YouTube did not return an upload session URI"
            ))
        })
}

enum ChunkOutcome {
    /// Bytes stored so far; keep going from here.
    Continue(u64),
    Done(Value),
}

async fn interpret(resp: reqwest::Response) -> Result<ChunkOutcome, (bool, GwsError)> {
    let status = resp.status();
    if status.as_u16() == 308 {
        let range = resp
            .headers()
            .get(reqwest::header::RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        return next_offset_from_range(range.as_deref())
            .map(ChunkOutcome::Continue)
            .map_err(|e| (false, e));
    }
    let text = resp.text().await.unwrap_or_default();
    if status.is_success() {
        let v: Value = serde_json::from_str(&text).map_err(|e| {
            (
                false,
                GwsError::Other(anyhow::anyhow!(
                    "Upload finished but the reply was not JSON: {e}"
                )),
            )
        })?;
        return Ok(ChunkOutcome::Done(v));
    }
    if status.as_u16() == 404 || status.as_u16() == 410 {
        return Err((
            false,
            GwsError::Other(anyhow::anyhow!(
                "The upload session expired ({status}); run the upload again"
            )),
        ));
    }
    let retryable = status.is_server_error() || status.as_u16() == 429;
    Err((retryable, crate::executor::api_error(status, &text)))
}

/// Asks the session how many bytes it has (used after a failed chunk).
async fn query_status(
    client: &reqwest::Client,
    token: &str,
    session: &str,
    total: u64,
) -> Result<ChunkOutcome, (bool, GwsError)> {
    let resp = client
        .put(session)
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_RANGE, format!("bytes */{total}"))
        .header(reqwest::header::CONTENT_LENGTH, "0")
        .send()
        .await
        .map_err(|e| (true, GwsError::Other(anyhow::anyhow!("{e}"))))?;
    interpret(resp).await
}

/// Sends the file to a resumable session in chunks, resuming after
/// transient failures. Calls `progress(sent, total)` after each chunk.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn upload_file(
    client: &reqwest::Client,
    token: &str,
    session: &str,
    path: &Path,
    total: u64,
    content_type: &str,
    chunk_size: u64,
    mut progress: impl FnMut(u64, u64),
) -> Result<Value, GwsError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Cannot open video: {e}")))?;
    let mut offset = 0u64;
    let mut failures = 0u32;
    loop {
        let len = chunk_size.min(total - offset);
        let chunk = read_chunk(&mut file, offset, len).await?;
        let sent = client
            .put(session)
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .header(
                reqwest::header::CONTENT_RANGE,
                content_range(offset, offset + len - 1, total),
            )
            .body(chunk)
            .send()
            .await;
        let outcome = match sent {
            Ok(resp) => interpret(resp).await,
            Err(e) => Err((true, GwsError::Other(anyhow::anyhow!("{e}")))),
        };
        let outcome = match outcome {
            Ok(o) => Ok(o),
            Err((true, err)) => {
                failures += 1;
                if failures > MAX_RETRIES {
                    return Err(err);
                }
                eprintln!(
                    "\nUpload interrupted ({err}); resuming in {}s (attempt {failures}/{MAX_RETRIES})",
                    backoff(failures).as_secs()
                );
                tokio::time::sleep(retry_delay(failures)).await;
                query_status(client, token, session, total).await
            }
            Err((false, err)) => return Err(err),
        };
        match outcome {
            Ok(ChunkOutcome::Done(v)) => {
                progress(total, total);
                return Ok(v);
            }
            Ok(ChunkOutcome::Continue(next)) => {
                if next > total {
                    return Err(GwsError::Other(anyhow::anyhow!(
                        "Upload server reported {next} bytes stored of {total}"
                    )));
                }
                if next > offset {
                    failures = 0;
                }
                offset = next;
                progress(offset, total);
                if offset == total {
                    // Everything is stored but no final reply yet: ask for it.
                    match query_status(client, token, session, total).await {
                        Ok(ChunkOutcome::Done(v)) => return Ok(v),
                        Ok(ChunkOutcome::Continue(n)) => offset = n.min(total.saturating_sub(1)),
                        Err((_, e)) => return Err(e),
                    }
                }
            }
            Err((_, e)) => return Err(e),
        }
    }
}

async fn set_thumbnail(
    client: &reqwest::Client,
    token: &str,
    upload_base: &str,
    video_id: &str,
    path: &Path,
) -> Result<(), GwsError> {
    let ct = thumbnail_content_type(path)?;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Cannot read thumbnail: {e}")))?;
    crate::executor::fetch_json(
        || {
            client
                .post(format!("{upload_base}/thumbnails/set"))
                .query(&[("videoId", video_id), ("uploadType", "media")])
                .bearer_auth(token)
                .header(reqwest::header::CONTENT_TYPE, ct)
                .body(bytes.clone())
        },
        "YouTube thumbnails.set",
    )
    .await?;
    Ok(())
}

async fn add_to_playlist(
    client: &reqwest::Client,
    token: &str,
    api_base: &str,
    playlist: &str,
    video_id: &str,
) -> Result<(), GwsError> {
    let body = json!({"snippet": {"playlistId": playlist, "resourceId": {"kind": "youtube#video", "videoId": video_id}}});
    crate::executor::fetch_json(
        || {
            client
                .post(format!("{api_base}/playlistItems"))
                .query(&[("part", "snippet")])
                .bearer_auth(token)
                .json(&body)
        },
        "YouTube playlistItems.insert",
    )
    .await?;
    Ok(())
}

async fn handle_upload(matches: &ArgMatches) -> Result<(), GwsError> {
    let o = parse_upload_options(matches, chrono::Utc::now())?;
    if matches.get_flag("dry-run") {
        let plan = dry_run_plan(&o, UPLOAD_BASE, API_BASE);
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).unwrap_or_default()
        );
        return Ok(());
    }
    let token = auth::get_token(&[SCOPE_UPLOAD])
        .await
        .map_err(|e| GwsError::Auth(format!("YouTube auth failed: {e}")))?;
    // Uploads can run for a long time: no overall request timeout.
    let client = crate::client::build_client()?;

    let session = start_session(&client, &token, UPLOAD_BASE, &o).await?;
    let started = Instant::now();
    let tty = std::io::stderr().is_terminal();
    let mut last_decile = 0u64;
    let video = upload_file(
        &client,
        &token,
        &session,
        &o.video,
        o.video_size,
        &o.content_type,
        CHUNK_SIZE,
        |sent, total| {
            let line = progress_line(sent, total, started.elapsed());
            if tty {
                eprint!("\r{line}   ");
            } else {
                let decile = (sent * 10).checked_div(total).unwrap_or(10);
                if decile > last_decile || sent == total {
                    last_decile = decile;
                    eprintln!("{line}");
                }
            }
        },
    )
    .await?;
    if tty {
        eprintln!();
    }

    let id = video
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut warnings: Vec<String> = Vec::new();
    let mut thumbnail_set = Value::Null;
    if let Some(t) = &o.thumbnail {
        match set_thumbnail(&client, &token, UPLOAD_BASE, &id, t).await {
            Ok(()) => thumbnail_set = json!(true),
            Err(e) => {
                thumbnail_set = json!(false);
                warnings.push(format!("Thumbnail not set: {e}"));
            }
        }
    }
    let mut playlist_added = Value::Null;
    if let Some(p) = &o.playlist {
        match add_to_playlist(&client, &token, API_BASE, p, &id).await {
            Ok(()) => playlist_added = json!(true),
            Err(e) => {
                playlist_added = json!(false);
                warnings.push(format!("Not added to playlist {p}: {e}"));
            }
        }
    }
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    let out = json!({
        "id": id,
        "url": format!("https://youtu.be/{id}"),
        "studio": format!("https://studio.youtube.com/video/{id}/edit"),
        "title": o.title,
        "privacy": video.pointer("/status/privacyStatus").cloned().unwrap_or(json!(o.privacy)),
        "uploadStatus": video.pointer("/status/uploadStatus").cloned().unwrap_or(Value::Null),
        "publishAt": o.publish_at,
        "bytes": o.video_size,
        "thumbnailSet": thumbnail_set,
        "playlistAdded": playlist_added,
        "warnings": warnings,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(())
}

// ── +videos ──────────────────────────────────────────────────────────

/// ISO 8601 duration (PT1H2M3S) as h:mm:ss or m:ss.
pub(crate) fn pretty_duration(iso: &str) -> String {
    let Some(rest) = iso.strip_prefix('P') else {
        return iso.to_string();
    };
    let (days, time) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };
    let mut secs: u64 = 0;
    let mut num = String::new();
    for c in days.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if c == 'D' {
                secs += num.parse::<u64>().unwrap_or(0) * 86_400;
            }
            num.clear();
        }
    }
    for c in time.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n = num.parse::<u64>().unwrap_or(0);
            secs += match c {
                'H' => n * 3600,
                'M' => n * 60,
                'S' => n,
                _ => 0,
            };
            num.clear();
        }
    }
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub(crate) fn video_rows(videos: &[Value]) -> Vec<Vec<Value>> {
    let s = |v: &Value, p: &str| {
        v.pointer(p)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let n = |v: &Value, p: &str| -> Value {
        v.pointer(p)
            .and_then(Value::as_str)
            .and_then(|x| x.parse::<u64>().ok())
            .map(Value::from)
            .unwrap_or(Value::String(String::new()))
    };
    videos
        .iter()
        .map(|v| {
            let id = s(v, "/id");
            let published = s(v, "/snippet/publishedAt");
            let scheduled = s(v, "/status/publishAt");
            let privacy = if scheduled.is_empty() {
                s(v, "/status/privacyStatus")
            } else {
                format!("{} (publishes {scheduled})", s(v, "/status/privacyStatus"))
            };
            vec![
                json!(published.get(..10).unwrap_or(&published)),
                json!(s(v, "/snippet/title")),
                json!(privacy),
                n(v, "/statistics/viewCount"),
                n(v, "/statistics/likeCount"),
                n(v, "/statistics/commentCount"),
                json!(pretty_duration(&s(v, "/contentDetails/duration"))),
                json!(format!("https://youtu.be/{id}")),
            ]
        })
        .collect()
}

async fn handle_videos(matches: &ArgMatches) -> Result<(), GwsError> {
    let format = match matches.get_one::<String>("format") {
        Some(f) => OutputFormat::from_str(f),
        None => OutputFormat::Table,
    };
    let limit = *matches.get_one::<u32>("limit").unwrap_or(&10) as usize;
    if matches.get_flag("dry-run") {
        let out = json!({"dry_run": true, "requests": [
            {"method": "GET", "url": format!("{API_BASE}/channels"), "query": {"part": "snippet,contentDetails", "mine": "true"}},
            {"method": "GET", "url": format!("{API_BASE}/playlistItems"), "query": {"part": "contentDetails", "playlistId": "<uploads playlist>", "maxResults": limit.min(50)}},
            {"method": "GET", "url": format!("{API_BASE}/videos"), "query": {"part": "snippet,status,statistics,contentDetails", "id": "<video IDs>"}},
        ]});
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return Ok(());
    }
    let token = auth::get_token(&[SCOPE_READ])
        .await
        .map_err(|e| GwsError::Auth(format!("YouTube auth failed: {e}")))?;
    let client = crate::client::shared_client()?;

    let channels = crate::executor::fetch_json(
        || {
            client
                .get(format!("{API_BASE}/channels"))
                .query(&[("part", "snippet,contentDetails"), ("mine", "true")])
                .bearer_auth(&token)
        },
        "YouTube channels.list",
    )
    .await?;
    let Some(channel) = channels
        .get("items")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    else {
        println!("This account has no YouTube channel.");
        return Ok(());
    };
    let channel_title = channel
        .pointer("/snippet/title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let uploads = channel
        .pointer("/contentDetails/relatedPlaylists/uploads")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut ids: Vec<String> = Vec::new();
    let mut page: Option<String> = None;
    while !uploads.is_empty() && ids.len() < limit {
        let want = (limit - ids.len()).min(50).to_string();
        let resp = crate::executor::fetch_json(
            || {
                let mut q = vec![
                    ("part", "contentDetails".to_string()),
                    ("playlistId", uploads.clone()),
                    ("maxResults", want.clone()),
                ];
                if let Some(p) = &page {
                    q.push(("pageToken", p.clone()));
                }
                client
                    .get(format!("{API_BASE}/playlistItems"))
                    .query(&q)
                    .bearer_auth(&token)
            },
            "YouTube playlistItems.list",
        )
        .await?;
        if let Some(items) = resp.get("items").and_then(Value::as_array) {
            ids.extend(
                items
                    .iter()
                    .filter_map(|i| i.pointer("/contentDetails/videoId").and_then(Value::as_str))
                    .map(str::to_string),
            );
        }
        page = resp
            .get("nextPageToken")
            .and_then(Value::as_str)
            .map(str::to_string);
        if page.is_none() {
            break;
        }
    }
    ids.truncate(limit);

    let mut videos: Vec<Value> = Vec::new();
    for batch in ids.chunks(50) {
        let joined = batch.join(",");
        let resp = crate::executor::fetch_json(
            || {
                client
                    .get(format!("{API_BASE}/videos"))
                    .query(&[
                        ("part", "snippet,status,statistics,contentDetails"),
                        ("id", joined.as_str()),
                    ])
                    .bearer_auth(&token)
            },
            "YouTube videos.list",
        )
        .await?;
        if let Some(items) = resp.get("items").and_then(Value::as_array) {
            videos.extend(items.iter().cloned());
        }
    }
    // videos.list doesn't keep request order; uploads are newest first.
    videos.sort_by(|a, b| {
        let p = |v: &Value| {
            v.pointer("/snippet/publishedAt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        p(b).cmp(&p(a))
    });

    let columns: Vec<String> = [
        "published",
        "title",
        "privacy",
        "views",
        "likes",
        "comments",
        "duration",
        "url",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let rows = video_rows(&videos);
    let title = format!("{channel_title} · {} most recent uploads", rows.len());
    let rendered = crate::formatter::render_report(
        &columns,
        &rows,
        &json!({"channel": channel_title, "uploadsPlaylist": uploads}),
        Some(&title),
        &format!("No uploads on the channel '{channel_title}' yet."),
        &format,
    );
    print!("{}", rendered.stdout);
    if let Some(n) = rendered.notice {
        eprintln!("{n}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn cmd() -> Command {
        let root = Command::new("gws")
            .arg(Arg::new("format").long("format").global(true))
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .global(true)
                    .action(ArgAction::SetTrue),
            );
        YouTubeHelper.inject_commands(root, &Default::default())
    }

    fn sample_opts(size: u64) -> UploadOptions {
        UploadOptions {
            video: PathBuf::from("film.mp4"),
            video_size: size,
            content_type: "video/mp4".into(),
            title: "Launch".into(),
            description: "About".into(),
            tags: vec!["mbprep".into()],
            privacy: "private".into(),
            publish_at: None,
            category: "22".into(),
            thumbnail: None,
            playlist: None,
            made_for_kids: false,
            notify: true,
        }
    }

    #[test]
    fn privacy_defaults_to_private_and_is_allowlisted() {
        let m = cmd()
            .try_get_matches_from(["gws", "+upload", "v.mp4", "--title", "t"])
            .unwrap();
        let m = m.subcommand_matches("+upload").unwrap();
        assert_eq!(m.get_one::<String>("privacy").unwrap(), "private");
        assert!(cmd()
            .try_get_matches_from([
                "gws",
                "+upload",
                "v.mp4",
                "--title",
                "t",
                "--privacy",
                "secret"
            ])
            .is_err());
        assert!(cmd()
            .try_get_matches_from(["gws", "+upload", "v.mp4"])
            .is_err());
    }

    #[test]
    fn title_rules() {
        assert_eq!(validate_title("  Hello  ").unwrap(), "Hello");
        assert!(validate_title("").is_err());
        assert!(validate_title(&"x".repeat(101)).is_err());
        assert!(validate_title(&"é".repeat(100)).is_ok());
        assert!(validate_title("a <b>").is_err());
        assert!(validate_title("bell\u{7}").is_err());
    }

    #[test]
    fn description_rules() {
        assert_eq!(read_description(None).unwrap(), "");
        assert_eq!(
            read_description(Some("line1\nline2")).unwrap(),
            "line1\nline2"
        );
        assert!(read_description(Some(&"x".repeat(5001))).is_err());
        assert!(read_description(Some("<script>")).is_err());
        assert!(read_description(Some("bad\u{1}")).is_err());
        assert!(read_description(Some("@../../etc/passwd")).is_err());
        assert!(read_description(Some("@does-not-exist.txt")).is_err());
    }

    #[test]
    #[serial_test::serial]
    fn description_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        std::fs::write("desc.txt", "From a file\n\n").unwrap();
        let r = read_description(Some("@desc.txt"));
        std::env::set_current_dir(cwd).unwrap();
        assert_eq!(r.unwrap(), "From a file");
    }

    #[test]
    fn tags_rules() {
        assert!(parse_tags(None).unwrap().is_empty());
        assert_eq!(
            parse_tags(Some("mbprep, medicine,MBPrep,,")).unwrap(),
            vec!["mbprep", "medicine"]
        );
        assert!(parse_tags(Some("a<b")).is_err());
        let many = (0..80)
            .map(|i| format!("tag{i:04}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_tags(Some(&many)).is_err());
    }

    #[test]
    fn ids_and_publish_at() {
        assert!(validate_youtube_id("PLabc_DEF-123", "--playlist").is_ok());
        assert!(validate_youtube_id("PL/../x", "--playlist").is_err());
        assert!(validate_youtube_id("", "--playlist").is_err());
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-23T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            validate_publish_at("2026-12-01T09:00:00+01:00", "private", now).unwrap(),
            "2026-12-01T08:00:00Z"
        );
        assert!(validate_publish_at("2026-11-01T09:00:00Z", "private", now).is_err());
        assert!(validate_publish_at("2026-12-01T09:00:00Z", "public", now).is_err());
        assert!(validate_publish_at("tomorrow", "private", now).is_err());
    }

    #[test]
    fn content_types() {
        assert_eq!(video_content_type(Path::new("a.MP4")), "video/mp4");
        assert_eq!(video_content_type(Path::new("a.mov")), "video/quicktime");
        assert_eq!(
            video_content_type(Path::new("a.bin")),
            "application/octet-stream"
        );
        assert_eq!(
            thumbnail_content_type(Path::new("c.JPG")).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            thumbnail_content_type(Path::new("c.png")).unwrap(),
            "image/png"
        );
        assert!(thumbnail_content_type(Path::new("c.gif")).is_err());
    }

    #[test]
    fn video_resource_shape() {
        let mut o = sample_opts(10);
        let v = build_video_resource(&o);
        assert_eq!(v["snippet"]["title"], "Launch");
        assert_eq!(v["snippet"]["tags"], json!(["mbprep"]));
        assert_eq!(v["snippet"]["categoryId"], "22");
        assert_eq!(v["status"]["privacyStatus"], "private");
        assert_eq!(v["status"]["selfDeclaredMadeForKids"], false);
        assert!(v["status"].get("publishAt").is_none());
        o.tags.clear();
        o.publish_at = Some("2026-12-01T08:00:00Z".into());
        let v = build_video_resource(&o);
        assert!(v["snippet"].get("tags").is_none());
        assert_eq!(v["status"]["publishAt"], "2026-12-01T08:00:00Z");
    }

    #[test]
    fn ranges_and_backoff() {
        assert_eq!(content_range(0, 9, 10), "bytes 0-9/10");
        assert_eq!(next_offset_from_range(None).unwrap(), 0);
        assert_eq!(
            next_offset_from_range(Some("bytes=0-262143")).unwrap(),
            262144
        );
        assert!(next_offset_from_range(Some("garbage")).is_err());
        assert_eq!(backoff(1), Duration::from_secs(2));
        assert_eq!(backoff(9), Duration::from_secs(32));
        assert_eq!(CHUNK_SIZE % (256 * 1024), 0);
    }

    #[test]
    fn progress_text() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
        let l = progress_line(50 * 1024 * 1024, 100 * 1024 * 1024, Duration::from_secs(10));
        assert!(l.contains(" 50.0%"), "{l}");
        assert!(l.contains("50.0 MB / 100.0 MB"), "{l}");
        assert!(l.contains("5.0 MB/s"), "{l}");
        assert!(l.contains("left"), "{l}");
        assert!(!progress_line(10, 10, Duration::from_secs(1)).contains("left"));
    }

    #[test]
    fn dry_run_lists_every_request() {
        let mut o = sample_opts(20 * 1024 * 1024);
        o.thumbnail = Some(PathBuf::from("cover.jpg"));
        o.playlist = Some("PL1".into());
        let p = dry_run_plan(&o, "https://up", "https://api");
        let r = p["requests"].as_array().unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(r[0]["method"], "POST");
        assert_eq!(r[0]["url"], "https://up/videos");
        assert_eq!(r[0]["query"]["uploadType"], "resumable");
        assert_eq!(r[0]["headers"]["X-Upload-Content-Length"], "20971520");
        assert_eq!(r[0]["body"]["status"]["privacyStatus"], "private");
        assert_eq!(r[1]["chunks"], 3);
        assert_eq!(r[1]["first_content_range"], "bytes 0-8388607/20971520");
        assert_eq!(r[2]["url"], "https://up/thumbnails/set");
        assert_eq!(r[3]["body"]["snippet"]["playlistId"], "PL1");
    }

    #[test]
    fn durations_and_rows() {
        assert_eq!(pretty_duration("PT1M5S"), "1:05");
        assert_eq!(pretty_duration("PT1H2M3S"), "1:02:03");
        assert_eq!(pretty_duration("P1DT1S"), "24:00:01");
        assert_eq!(pretty_duration("PT45S"), "0:45");
        assert_eq!(pretty_duration("odd"), "odd");
        let rows = video_rows(&[json!({
            "id": "abc",
            "snippet": {"title": "Film", "publishedAt": "2026-11-20T10:00:00Z"},
            "status": {"privacyStatus": "private", "publishAt": "2026-12-01T08:00:00Z"},
            "statistics": {"viewCount": "12", "likeCount": "3"},
            "contentDetails": {"duration": "PT2M"}
        })]);
        assert_eq!(rows[0][0], "2026-11-20");
        assert_eq!(rows[0][2], "private (publishes 2026-12-01T08:00:00Z)");
        assert_eq!(rows[0][3], 12);
        assert_eq!(rows[0][5], "");
        assert_eq!(rows[0][6], "2:00");
        assert_eq!(rows[0][7], "https://youtu.be/abc");
    }

    // ── resumable upload against a local fake server ──────────────────

    /// A fake resumable endpoint: stores bytes, answers 308 until complete,
    /// and fails the request numbers listed in `fail_on` with a 503.
    async fn fake_upload_server(fail_on: Vec<usize>) -> (String, Arc<Mutex<Vec<u8>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stored = Arc::new(Mutex::new(Vec::<u8>::new()));
        let store = stored.clone();
        tokio::spawn(async move {
            let mut n = 0usize;
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                n += 1;
                // Read headers, then the body by Content-Length.
                let mut buf = Vec::new();
                let mut tmp = [0u8; 65536];
                let header_end;
                loop {
                    let k = sock.read(&mut tmp).await.unwrap();
                    buf.extend_from_slice(&tmp[..k]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = i + 4;
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map(|v| v.trim().parse().unwrap())
                    .unwrap_or(0);
                while buf.len() < header_end + len {
                    let k = sock.read(&mut tmp).await.unwrap();
                    buf.extend_from_slice(&tmp[..k]);
                }
                let body = buf[header_end..header_end + len].to_vec();
                let total: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-range:"))
                    .and_then(|v| v.trim().rsplit('/').next().map(|t| t.parse().unwrap()))
                    .unwrap_or(0);
                let resp = if fail_on.contains(&n) {
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    let mut s = store.lock().unwrap();
                    if !body.is_empty() {
                        let start: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-range: bytes "))
                            .and_then(|v| v.split('-').next().map(|x| x.parse().unwrap()))
                            .unwrap();
                        s.truncate(start);
                        s.extend_from_slice(&body);
                    }
                    if s.len() == total && total > 0 {
                        let json = r#"{"id":"vid123","status":{"privacyStatus":"private","uploadStatus":"uploaded"}}"#;
                        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}", json.len())
                    } else if s.is_empty() {
                        "HTTP/1.1 308 Resume Incomplete\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                    } else {
                        format!("HTTP/1.1 308 Resume Incomplete\r\nRange: bytes=0-{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", s.len() - 1)
                    }
                };
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        (format!("http://{addr}/session"), stored)
    }

    fn temp_video(bytes: usize) -> (tempfile::TempDir, PathBuf, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.mp4");
        let data: Vec<u8> = (0..bytes).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        (dir, path, data)
    }

    #[tokio::test]
    async fn upload_file_sends_all_chunks() {
        let (_dir, path, data) = temp_video(700_000);
        let (session, stored) = fake_upload_server(vec![]).await;
        let client = reqwest::Client::new();
        let mut seen = Vec::new();
        let v = upload_file(
            &client,
            "tok",
            &session,
            &path,
            data.len() as u64,
            "video/mp4",
            262_144,
            |s, t| seen.push((s, t)),
        )
        .await
        .unwrap();
        assert_eq!(v["id"], "vid123");
        assert_eq!(*stored.lock().unwrap(), data);
        assert_eq!(seen.last().unwrap(), &(700_000, 700_000));
        assert!(seen.len() >= 3);
    }

    #[tokio::test]
    async fn upload_file_resumes_after_server_error() {
        let (_dir, path, data) = temp_video(600_000);
        // Second request (the second chunk) fails; the status query resumes it.
        let (session, stored) = fake_upload_server(vec![2]).await;
        let client = reqwest::Client::new();
        let v = upload_file(
            &client,
            "tok",
            &session,
            &path,
            data.len() as u64,
            "video/mp4",
            262_144,
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(v["id"], "vid123");
        assert_eq!(*stored.lock().unwrap(), data);
    }
}

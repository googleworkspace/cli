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

//! A small Drive v3 client shared by the `drive` and `docs` helpers.
//!
//! The helpers in this module need several calls per command (walk a folder
//! tree, upload, update in place, trash), so they talk to the REST API
//! directly instead of going through `executor::execute_method`, which prints
//! every response. Every request carries a fresh token from [`crate::auth`],
//! so `--profile` and the other credential sources apply unchanged.

use crate::auth;
use crate::error::GwsError;
use crate::validate::encode_path_segment;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub(crate) const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";
pub(crate) const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
pub(crate) const DOC_MIME: &str = "application/vnd.google-apps.document";
pub(crate) const GOOGLE_APPS_PREFIX: &str = "application/vnd.google-apps.";

const API_BASE: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD_BASE: &str = "https://www.googleapis.com/upload/drive/v3";

/// Fields requested for every file resource.
pub(crate) const FILE_FIELDS: &str =
    "id,name,mimeType,md5Checksum,size,parents,driveId,trashed,webViewLink,modifiedTime";

/// Drive documents `uploadType=multipart` for payloads up to 5 MB; anything
/// larger goes through the resumable protocol.
const MULTIPART_LIMIT: u64 = 5 * 1024 * 1024;

/// Attempts per request, including the first one.
const MAX_ATTEMPTS: u32 = 5;

/// Re-fetch the access token after this long. Tokens live for an hour; a long
/// sync must not run into an expired one.
const TOKEN_TTL: Duration = Duration::from_secs(40 * 60);

/// A Drive file resource, reduced to the fields the helpers use.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveFile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5_checksum: Option<String>,
    /// Drive returns `size` as a decimal string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trashed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_view_link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_time: Option<String>,
}

impl DriveFile {
    pub fn is_folder(&self) -> bool {
        self.mime_type == FOLDER_MIME
    }

    /// Google-native files (Docs, Sheets, …) have no binary content, so no
    /// size and no checksum.
    pub fn is_google_native(&self) -> bool {
        self.mime_type.starts_with(GOOGLE_APPS_PREFIX)
    }

    pub fn size_bytes(&self) -> Option<u64> {
        self.size.as_deref().and_then(|s| s.parse().ok())
    }
}

/// A shared drive, as returned by `drives.list`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SharedDrive {
    pub id: String,
    pub name: String,
}

/// The payload of an upload.
pub(crate) enum Media<'a> {
    /// Stream a file from disk.
    File { path: &'a Path, size: u64 },
    /// Send bytes already in memory.
    Bytes(&'a [u8]),
}

impl Media<'_> {
    fn len(&self) -> u64 {
        match self {
            Media::File { size, .. } => *size,
            Media::Bytes(b) => b.len() as u64,
        }
    }

    async fn body(&self) -> Result<reqwest::Body, GwsError> {
        match self {
            Media::File { path, .. } => {
                let file = tokio::fs::File::open(path).await.map_err(|e| {
                    GwsError::Validation(format!("Failed to open '{}': {e}", path.display()))
                })?;
                Ok(reqwest::Body::wrap_stream(
                    tokio_util::io::ReaderStream::new(file),
                ))
            }
            Media::Bytes(b) => Ok(reqwest::Body::from(b.to_vec())),
        }
    }
}

/// Authenticated Drive client. Cheap to share by reference across tasks.
pub(crate) struct DriveClient {
    http: reqwest::Client,
    scopes: Vec<String>,
    token: Mutex<Option<(String, Instant)>>,
}

impl DriveClient {
    /// Builds a client and fetches a token up front, so an auth problem
    /// surfaces before any work starts.
    pub async fn connect(scopes: &[&str]) -> Result<Self, GwsError> {
        let client = Self {
            http: crate::client::shared_client()?,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            token: Mutex::new(None),
        };
        client.access_token().await?;
        Ok(client)
    }

    async fn access_token(&self) -> Result<String, GwsError> {
        let mut guard = self.token.lock().await;
        if let Some((token, at)) = guard.as_ref() {
            if at.elapsed() < TOKEN_TTL {
                return Ok(token.clone());
            }
        }
        let scopes: Vec<&str> = self.scopes.iter().map(String::as_str).collect();
        let token = auth::get_token(&scopes)
            .await
            .map_err(|e| GwsError::Auth(format!("Drive auth failed: {e:#}")))?;
        *guard = Some((token.clone(), Instant::now()));
        Ok(token)
    }

    /// Sends a request built by `build`, retrying rate limits, server errors
    /// and dropped connections with exponential backoff. Returns the response
    /// once it is a success, or the API error otherwise.
    async fn send<F, Fut>(&self, what: &str, build: F) -> Result<reqwest::Response, GwsError>
    where
        F: Fn(reqwest::Client, String) -> Fut,
        Fut: std::future::Future<Output = Result<reqwest::RequestBuilder, GwsError>>,
    {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let token = self.access_token().await?;
            let request = build(self.http.clone(), token).await?;
            match request.send().await {
                Ok(resp) if resp.status().is_success() => return Ok(resp),
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let body = resp.text().await.unwrap_or_default();
                    let err = api_error(status, &body, what);
                    if attempt < MAX_ATTEMPTS && is_retryable(status, &err) {
                        tokio::time::sleep(backoff(attempt)).await;
                        continue;
                    }
                    return Err(err);
                }
                Err(e) if attempt < MAX_ATTEMPTS && is_transient(&e) => {
                    tokio::time::sleep(backoff(attempt)).await;
                }
                Err(e) => {
                    return Err(GwsError::Other(anyhow::anyhow!("{what}: {e}")));
                }
            }
        }
    }

    async fn send_json(
        &self,
        what: &str,
        build: impl Fn(reqwest::Client, String) -> reqwest::RequestBuilder,
    ) -> Result<Value, GwsError> {
        let resp = self
            .send(what, |http, token| {
                let rb = build(http, token);
                async move { Ok(rb) }
            })
            .await?;
        resp.json::<Value>()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("{what}: invalid JSON response: {e}")))
    }

    /// GET any Google API URL that returns JSON (used for the Docs API too).
    pub async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> Result<Value, GwsError> {
        self.send_json(&format!("GET {url}"), |http, token| {
            http.get(url).bearer_auth(token).query(query)
        })
        .await
    }

    /// `files.get`, following shortcuts is left to the caller.
    pub async fn get_file(&self, id: &str) -> Result<DriveFile, GwsError> {
        let url = format!("{API_BASE}/files/{}", encode_path_segment(id));
        let v = self
            .send_json("files.get", |http, token| {
                http.get(&url)
                    .bearer_auth(token)
                    .query(&[("fields", FILE_FIELDS), ("supportsAllDrives", "true")])
            })
            .await?;
        parse_file(v)
    }

    /// `files.list` over every page.
    pub async fn list(&self, q: &str, drive_id: Option<&str>) -> Result<Vec<DriveFile>, GwsError> {
        let fields = format!("nextPageToken,files({FILE_FIELDS})");
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut query: Vec<(&str, String)> = vec![
                ("q", q.to_string()),
                ("fields", fields.clone()),
                ("pageSize", "1000".to_string()),
                ("supportsAllDrives", "true".to_string()),
                ("includeItemsFromAllDrives", "true".to_string()),
            ];
            match drive_id {
                Some(d) => {
                    query.push(("corpora", "drive".to_string()));
                    query.push(("driveId", d.to_string()));
                }
                None => query.push(("corpora", "user".to_string())),
            }
            if let Some(t) = &page_token {
                query.push(("pageToken", t.clone()));
            }
            let url = format!("{API_BASE}/files");
            let v = self
                .send_json("files.list", |http, token| {
                    http.get(&url).bearer_auth(token).query(&query)
                })
                .await?;
            if let Some(files) = v.get("files").and_then(Value::as_array) {
                for f in files {
                    out.push(parse_file(f.clone())?);
                }
            }
            page_token = v
                .get("nextPageToken")
                .and_then(Value::as_str)
                .map(str::to_string);
            if page_token.is_none() {
                return Ok(out);
            }
        }
    }

    /// Every non-trashed child of `parent`.
    pub async fn children(
        &self,
        parent: &str,
        drive_id: Option<&str>,
    ) -> Result<Vec<DriveFile>, GwsError> {
        let q = format!("'{}' in parents and trashed = false", escape_query(parent));
        self.list(&q, drive_id).await
    }

    /// Children of `parent` named exactly `name`.
    pub async fn children_named(
        &self,
        parent: &str,
        name: &str,
        drive_id: Option<&str>,
    ) -> Result<Vec<DriveFile>, GwsError> {
        let q = format!(
            "'{}' in parents and name = '{}' and trashed = false",
            escape_query(parent),
            escape_query(name)
        );
        self.list(&q, drive_id).await
    }

    /// Shared drives with exactly this name.
    pub async fn shared_drives_named(&self, name: &str) -> Result<Vec<SharedDrive>, GwsError> {
        let q = format!("name = '{}'", escape_query(name));
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut query: Vec<(&str, String)> = vec![
                ("q", q.clone()),
                ("pageSize", "100".to_string()),
                ("fields", "nextPageToken,drives(id,name)".to_string()),
            ];
            if let Some(t) = &page_token {
                query.push(("pageToken", t.clone()));
            }
            let url = format!("{API_BASE}/drives");
            let v = self
                .send_json("drives.list", |http, token| {
                    http.get(&url).bearer_auth(token).query(&query)
                })
                .await?;
            if let Some(drives) = v.get("drives").and_then(Value::as_array) {
                for d in drives {
                    out.push(serde_json::from_value(d.clone()).map_err(|e| {
                        GwsError::Other(anyhow::anyhow!("drives.list: unexpected drive: {e}"))
                    })?);
                }
            }
            page_token = v
                .get("nextPageToken")
                .and_then(Value::as_str)
                .map(str::to_string);
            if page_token.is_none() {
                return Ok(out);
            }
        }
    }

    pub async fn create_folder(&self, name: &str, parent: &str) -> Result<DriveFile, GwsError> {
        let url = format!("{API_BASE}/files");
        let body = json!({ "name": name, "mimeType": FOLDER_MIME, "parents": [parent] });
        let v = self
            .send_json("files.create (folder)", |http, token| {
                http.post(&url)
                    .bearer_auth(token)
                    .query(&[("fields", FILE_FIELDS), ("supportsAllDrives", "true")])
                    .json(&body)
            })
            .await?;
        parse_file(v)
    }

    /// Creates a file with content. `metadata` may set `mimeType` to a Google
    /// type, in which case Drive converts the media (e.g. Markdown → Doc).
    pub async fn create_with_media(
        &self,
        metadata: &Value,
        media: Media<'_>,
        media_mime: &str,
    ) -> Result<DriveFile, GwsError> {
        self.upload(reqwest::Method::POST, None, metadata, media, media_mime)
            .await
    }

    /// Replaces a file's content in place: the id, the share links and the
    /// revision history all stay.
    pub async fn update_media(
        &self,
        id: &str,
        media: Media<'_>,
        media_mime: &str,
    ) -> Result<DriveFile, GwsError> {
        self.upload(
            reqwest::Method::PATCH,
            Some(id),
            &json!({}),
            media,
            media_mime,
        )
        .await
    }

    async fn upload(
        &self,
        method: reqwest::Method,
        id: Option<&str>,
        metadata: &Value,
        media: Media<'_>,
        media_mime: &str,
    ) -> Result<DriveFile, GwsError> {
        let url = match id {
            Some(id) => format!("{UPLOAD_BASE}/files/{}", encode_path_segment(id)),
            None => format!("{UPLOAD_BASE}/files"),
        };
        let what = if id.is_some() {
            "files.update (media)"
        } else {
            "files.create (media)"
        };
        let media_ref = &media;
        let resp = if media.len() <= MULTIPART_LIMIT {
            self.send(what, move |http, token| {
                let url = url.clone();
                let method = method.clone();
                let media = media_ref;
                async move {
                    let (body, content_type, len) =
                        multipart_body(metadata, media, media_mime).await?;
                    Ok(http
                        .request(method, url)
                        .bearer_auth(token)
                        .query(&[
                            ("uploadType", "multipart"),
                            ("fields", FILE_FIELDS),
                            ("supportsAllDrives", "true"),
                        ])
                        .header(reqwest::header::CONTENT_TYPE, content_type)
                        .header(reqwest::header::CONTENT_LENGTH, len)
                        .body(body))
                }
            })
            .await?
        } else {
            // Resumable: open a session, then send the whole file in one PUT.
            let session = self
                .send(what, |http, token| {
                    let rb = http
                        .request(method.clone(), &url)
                        .bearer_auth(token)
                        .query(&[
                            ("uploadType", "resumable"),
                            ("fields", FILE_FIELDS),
                            ("supportsAllDrives", "true"),
                        ])
                        .header("X-Upload-Content-Type", media_mime)
                        .header("X-Upload-Content-Length", media.len())
                        .json(metadata);
                    async move { Ok(rb) }
                })
                .await?;
            let location = session
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
                .ok_or_else(|| {
                    GwsError::Other(anyhow::anyhow!("{what}: no resumable session URL returned"))
                })?;
            self.send(what, move |http, token| {
                let location = location.clone();
                let media = media_ref;
                async move {
                    Ok(http
                        .put(location)
                        .bearer_auth(token)
                        .header(reqwest::header::CONTENT_TYPE, media_mime)
                        .header(reqwest::header::CONTENT_LENGTH, media.len())
                        .body(media.body().await?))
                }
            })
            .await?
        };
        let v = resp
            .json::<Value>()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("{what}: invalid JSON response: {e}")))?;
        parse_file(v)
    }

    /// Moves a file or folder to the trash. Never deletes permanently.
    pub async fn trash(&self, id: &str) -> Result<DriveFile, GwsError> {
        let url = format!("{API_BASE}/files/{}", encode_path_segment(id));
        let v = self
            .send_json("files.update (trash)", |http, token| {
                http.patch(&url)
                    .bearer_auth(token)
                    .query(&[("fields", FILE_FIELDS), ("supportsAllDrives", "true")])
                    .json(&json!({ "trashed": true }))
            })
            .await?;
        parse_file(v)
    }

    /// `files.export` of a Google-native file to `mime`.
    pub async fn export(&self, id: &str, mime: &str) -> Result<Vec<u8>, GwsError> {
        let url = format!("{API_BASE}/files/{}/export", encode_path_segment(id));
        let resp = self
            .send("files.export", |http, token| {
                let rb = http
                    .get(&url)
                    .bearer_auth(token)
                    .query(&[("mimeType", mime)]);
                async move { Ok(rb) }
            })
            .await?;
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| GwsError::Other(anyhow::anyhow!("files.export: {e}")))
    }
}

async fn multipart_body(
    metadata: &Value,
    media: &Media<'_>,
    media_mime: &str,
) -> Result<(reqwest::Body, String, u64), GwsError> {
    match media {
        Media::File { path, size } => {
            let path_str = path.to_str().ok_or_else(|| {
                GwsError::Validation(format!("Path is not valid UTF-8: {}", path.display()))
            })?;
            crate::executor::build_multipart_stream(
                &Some(metadata.clone()),
                path_str,
                *size,
                media_mime,
            )
        }
        Media::Bytes(bytes) => {
            let boundary = format!("gws_boundary_{:016x}", rand::random::<u64>());
            let mut body = format!(
                "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n\
                 --{boundary}\r\nContent-Type: {media_mime}\r\n\r\n"
            )
            .into_bytes();
            body.extend_from_slice(bytes);
            body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            let len = body.len() as u64;
            Ok((
                reqwest::Body::from(body),
                format!("multipart/related; boundary={boundary}"),
                len,
            ))
        }
    }
}

fn parse_file(v: Value) -> Result<DriveFile, GwsError> {
    serde_json::from_value(v)
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Unexpected Drive file resource: {e}")))
}

/// Escapes a value for a single-quoted string in a Drive `q` expression.
pub(crate) fn escape_query(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Builds a [`GwsError::Api`] from an error response body.
pub(crate) fn api_error(status: u16, body: &str, what: &str) -> GwsError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let err = parsed.as_ref().and_then(|v| v.get("error"));
    let message = err
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                format!("HTTP {status}")
            } else {
                trimmed.chars().take(500).collect()
            }
        });
    let reason = err
        .and_then(|e| e.get("errors"))
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|e| e.get("reason"))
        .and_then(Value::as_str)
        .or_else(|| err.and_then(|e| e.get("status")).and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    GwsError::Api {
        code: status,
        message: format!("{what}: {message}"),
        reason,
        enable_url: None,
    }
}

/// Rate limits and server-side failures are worth another attempt; anything
/// else (bad request, not found, permission) will fail the same way again.
pub(crate) fn is_retryable(status: u16, err: &GwsError) -> bool {
    if status == 429 || status >= 500 {
        return true;
    }
    if status == 403 {
        if let GwsError::Api { reason, .. } = err {
            return matches!(
                reason.as_str(),
                "rateLimitExceeded" | "userRateLimitExceeded" | "sharingRateLimitExceeded"
            );
        }
    }
    false
}

fn is_transient(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout() || e.is_request() || e.is_body()
}

fn backoff(attempt: u32) -> Duration {
    // 1s, 2s, 4s, 8s, plus up to 500 ms of jitter.
    let base = 1u64 << (attempt - 1).min(4);
    Duration::from_millis(base * 1000 + rand::random::<u64>() % 500)
}

/// The reason string of an API error, if it is one.
pub(crate) fn error_reason(err: &GwsError) -> Option<&str> {
    match err {
        GwsError::Api { reason, .. } => Some(reason.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_query_escapes_quotes_and_backslashes() {
        assert_eq!(escape_query("plain"), "plain");
        assert_eq!(escape_query("it's"), "it\\'s");
        assert_eq!(escape_query("a\\b"), "a\\\\b");
        assert_eq!(escape_query("x' or name != '"), "x\\' or name != \\'");
    }

    #[test]
    fn api_error_reads_google_error_shape() {
        let body = r#"{"error":{"code":403,"message":"Rate Limit Exceeded","errors":[{"reason":"userRateLimitExceeded"}]}}"#;
        let err = api_error(403, body, "files.list");
        match &err {
            GwsError::Api {
                code,
                message,
                reason,
                ..
            } => {
                assert_eq!(*code, 403);
                assert_eq!(message, "files.list: Rate Limit Exceeded");
                assert_eq!(reason, "userRateLimitExceeded");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(is_retryable(403, &err));
    }

    #[test]
    fn api_error_falls_back_to_body_and_status() {
        let err = api_error(502, "", "files.get");
        assert!(err.to_string().contains("HTTP 502"));
        assert!(is_retryable(502, &err));
        let err = api_error(400, "bad things", "files.get");
        assert!(err.to_string().contains("bad things"));
        assert!(!is_retryable(400, &err));
    }

    #[test]
    fn permission_errors_are_not_retried() {
        let body = r#"{"error":{"code":403,"message":"Insufficient permissions","errors":[{"reason":"insufficientFilePermissions"}]}}"#;
        let err = api_error(403, body, "files.update");
        assert!(!is_retryable(403, &err));
        assert_eq!(error_reason(&err), Some("insufficientFilePermissions"));
        assert!(!is_retryable(404, &api_error(404, "", "files.get")));
        assert!(is_retryable(429, &api_error(429, "", "files.get")));
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert!(backoff(1) >= Duration::from_secs(1) && backoff(1) < Duration::from_secs(2));
        assert!(backoff(3) >= Duration::from_secs(4));
        assert!(backoff(10) < Duration::from_secs(17));
    }

    #[test]
    fn drive_file_helpers() {
        let f: DriveFile = serde_json::from_value(json!({
            "id": "1", "name": "a.png", "mimeType": "image/png", "size": "42", "md5Checksum": "abc"
        }))
        .unwrap();
        assert_eq!(f.size_bytes(), Some(42));
        assert!(!f.is_folder());
        assert!(!f.is_google_native());
        let d: DriveFile =
            serde_json::from_value(json!({"id": "2", "name": "d", "mimeType": FOLDER_MIME}))
                .unwrap();
        assert!(d.is_folder());
        assert!(d.is_google_native());
        assert_eq!(d.size_bytes(), None);
    }

    #[tokio::test]
    async fn multipart_bytes_body_has_metadata_and_media() {
        let media = Media::Bytes(b"# hi");
        let (_body, ct, len) = multipart_body(&json!({"name": "x"}), &media, "text/markdown")
            .await
            .unwrap();
        assert!(ct.starts_with("multipart/related; boundary=gws_boundary_"));
        assert!(len > 4);
    }
}

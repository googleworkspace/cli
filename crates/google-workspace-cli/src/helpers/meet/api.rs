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

//! Thin HTTP layer for the Meet helpers.
//!
//! Every helper talks to Google through the [`MeetApi`] trait so the
//! orchestration logic (record resolution, paging, joining participants to
//! transcript entries, matching Calendar events) can be unit-tested against
//! canned responses without a network.

use crate::error::GwsError;
use async_trait::async_trait;
use serde_json::Value;

pub(super) const MEET_BASE: &str = "https://meet.googleapis.com/v2";
pub(super) const CALENDAR_BASE: &str = "https://www.googleapis.com/calendar/v3";
pub(super) const DRIVE_BASE: &str = "https://www.googleapis.com/drive/v3";

/// Upper bound on pages fetched by [`list_all`], so a misbehaving
/// `nextPageToken` can never loop forever.
pub(super) const MAX_PAGES: usize = 200;

/// Query string pairs. Values are encoded by reqwest's `.query()`.
pub(super) type Query = Vec<(&'static str, String)>;

// async_trait marks the boxed futures #[must_use]; newer clippy flags that.
#[allow(clippy::double_must_use)]
#[async_trait]
pub(super) trait MeetApi: Send + Sync {
    /// GET a URL and parse the body as JSON.
    async fn get_json(&self, url: &str, query: &Query) -> Result<Value, GwsError>;
    /// GET a URL and return the body as text (used for Drive exports).
    async fn get_text(&self, url: &str, query: &Query) -> Result<String, GwsError>;
    /// POST a JSON body and parse the response as JSON.
    async fn post_json(&self, url: &str, query: &Query, body: &Value) -> Result<Value, GwsError>;
}

/// The real implementation, backed by reqwest and a bearer token.
pub(super) struct HttpApi {
    pub(super) client: reqwest::Client,
    pub(super) token: String,
}

impl HttpApi {
    pub(super) fn new(token: String) -> Result<Self, GwsError> {
        Ok(Self {
            client: crate::client::build_client()?,
            token,
        })
    }

    async fn send(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, GwsError> {
        let resp = crate::client::send_with_retry(build)
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("HTTP request failed: {e}")))?;
        check_status(resp).await
    }
}

/// Turn a non-2xx response into the same structured [`GwsError::Api`] the
/// Discovery commands produce.
pub(super) async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response, GwsError> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = crate::auth::response_text_or_placeholder(resp.text().await);
    crate::executor::handle_error_response(status, &body, &crate::executor::AuthMethod::OAuth)
}

#[async_trait]
impl MeetApi for HttpApi {
    async fn get_json(&self, url: &str, query: &Query) -> Result<Value, GwsError> {
        let resp = self
            .send(|| self.client.get(url).query(query).bearer_auth(&self.token))
            .await?;
        resp.json()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse response: {e}")))
    }

    async fn get_text(&self, url: &str, query: &Query) -> Result<String, GwsError> {
        let resp = self
            .send(|| self.client.get(url).query(query).bearer_auth(&self.token))
            .await?;
        resp.text()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to read response: {e}")))
    }

    async fn post_json(&self, url: &str, query: &Query, body: &Value) -> Result<Value, GwsError> {
        let resp = self
            .send(|| {
                self.client
                    .post(url)
                    .query(query)
                    .bearer_auth(&self.token)
                    .json(body)
            })
            .await?;
        resp.json()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse response: {e}")))
    }
}

/// Fetch every page of a list endpoint and concatenate the `key` arrays.
///
/// Stops after `max_items` items (when given) or [`MAX_PAGES`] pages.
pub(super) async fn list_all(
    api: &dyn MeetApi,
    url: &str,
    key: &str,
    base_query: &Query,
    max_items: Option<usize>,
) -> Result<Vec<Value>, GwsError> {
    let mut out = Vec::new();
    let mut page_token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut query = base_query.clone();
        if let Some(t) = &page_token {
            query.push(("pageToken", t.clone()));
        }
        let page = api.get_json(url, &query).await?;
        if let Some(items) = page.get(key).and_then(Value::as_array) {
            out.extend(items.iter().cloned());
        }
        if let Some(max) = max_items {
            if out.len() >= max {
                out.truncate(max);
                break;
            }
        }
        page_token = page
            .get("nextPageToken")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        if page_token.is_none() {
            break;
        }
    }
    Ok(out)
}

/// Export a Google Doc (or any Workspace file) as plain text through Drive.
pub(super) async fn export_doc_text(api: &dyn MeetApi, file_id: &str) -> Result<String, GwsError> {
    let url = format!(
        "{DRIVE_BASE}/files/{}/export",
        crate::validate::encode_path_segment(file_id)
    );
    let text = api
        .get_text(&url, &vec![("mimeType", "text/plain".to_string())])
        .await?;
    // Drive prefixes text exports with a UTF-8 BOM.
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

/// Build the Meet API URL for a resource name such as
/// `conferenceRecords/abc/participants`. Each segment is percent-encoded.
pub(super) fn meet_url(resource: &str) -> String {
    format!(
        "{MEET_BASE}/{}",
        crate::validate::encode_path_preserving_slashes(resource)
    )
}

#[cfg(test)]
pub(super) mod fake {
    //! A scripted [`MeetApi`] for tests: responses are keyed by URL plus an
    //! optional `pageToken`, and every request is recorded.
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeApi {
        pub json: HashMap<String, Value>,
        pub text: HashMap<String, String>,
        pub calls: Mutex<Vec<(String, Query)>>,
        pub posts: Mutex<Vec<(String, Value)>>,
    }

    fn key(url: &str, query: &Query) -> String {
        match query.iter().find(|(k, _)| *k == "pageToken") {
            Some((_, t)) => format!("{url}#{t}"),
            None => url.to_string(),
        }
    }

    impl FakeApi {
        pub fn with(mut self, url: &str, v: Value) -> Self {
            self.json.insert(url.to_string(), v);
            self
        }
        pub fn with_text(mut self, url: &str, t: &str) -> Self {
            self.text.insert(url.to_string(), t.to_string());
            self
        }
        pub fn queries_for(&self, url: &str) -> Vec<Query> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(u, _)| u == url)
                .map(|(_, q)| q.clone())
                .collect()
        }
    }

    fn not_found(url: &str) -> GwsError {
        GwsError::Api {
            code: 404,
            message: format!("fake: no response for {url}"),
            reason: "notFound".to_string(),
            enable_url: None,
        }
    }

    #[async_trait]
    impl MeetApi for FakeApi {
        async fn get_json(&self, url: &str, query: &Query) -> Result<Value, GwsError> {
            self.calls
                .lock()
                .unwrap()
                .push((url.to_string(), query.clone()));
            self.json
                .get(&key(url, query))
                .cloned()
                .ok_or_else(|| not_found(url))
        }
        async fn get_text(&self, url: &str, query: &Query) -> Result<String, GwsError> {
            self.calls
                .lock()
                .unwrap()
                .push((url.to_string(), query.clone()));
            self.text.get(url).cloned().ok_or_else(|| not_found(url))
        }
        async fn post_json(
            &self,
            url: &str,
            _query: &Query,
            body: &Value,
        ) -> Result<Value, GwsError> {
            self.posts
                .lock()
                .unwrap()
                .push((url.to_string(), body.clone()));
            self.json.get(url).cloned().ok_or_else(|| not_found(url))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeApi;
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn list_all_follows_page_tokens() {
        let api = FakeApi::default()
            .with("u", json!({"items": [1, 2], "nextPageToken": "p2"}))
            .with("u#p2", json!({"items": [3]}));
        let all = list_all(&api, "u", "items", &vec![], None).await.unwrap();
        assert_eq!(all, vec![json!(1), json!(2), json!(3)]);
    }

    #[tokio::test]
    async fn list_all_respects_max_items() {
        let api = FakeApi::default()
            .with("u", json!({"items": [1, 2], "nextPageToken": "p2"}))
            .with("u#p2", json!({"items": [3]}));
        let all = list_all(&api, "u", "items", &vec![], Some(1))
            .await
            .unwrap();
        assert_eq!(all, vec![json!(1)]);
        assert_eq!(api.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn list_all_handles_missing_key_and_empty_token() {
        let api = FakeApi::default().with("u", json!({"nextPageToken": ""}));
        let all = list_all(&api, "u", "items", &vec![], None).await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn list_all_propagates_errors() {
        let api = FakeApi::default();
        assert!(list_all(&api, "missing", "items", &vec![], None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn export_doc_text_strips_bom_and_encodes_id() {
        let url = format!("{DRIVE_BASE}/files/a%2Fb/export");
        let api = FakeApi::default().with_text(&url, "\u{feff}hello");
        assert_eq!(export_doc_text(&api, "a/b").await.unwrap(), "hello");
        let q = api.queries_for(&url);
        assert_eq!(q[0], vec![("mimeType", "text/plain".to_string())]);
    }

    #[test]
    fn meet_url_keeps_slashes() {
        assert_eq!(
            meet_url("conferenceRecords/abc/participants"),
            format!("{MEET_BASE}/conferenceRecords/abc/participants")
        );
    }

    #[test]
    fn http_api_builds() {
        assert!(HttpApi::new("t".to_string()).is_ok());
    }
}

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

//! Notion tool. Wraps a small, frequently-used subset of the Notion REST
//! API: search, retrieve a page's properties, and append plain-text blocks
//! to a page.
//!
//! Authentication is a Notion integration token (`NOTION_API_KEY` or
//! `NOTION_TOKEN`). Users invite the integration to the pages/databases
//! they want the agent to touch.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::tool::{Tool, ToolError};

const NOTION_API_VERSION: &str = "2022-06-28";
const NOTION_BASE: &str = "https://api.notion.com/v1";

pub struct NotionTool {
    client: reqwest::Client,
    token: String,
}

impl NotionTool {
    pub fn from_env() -> Option<Self> {
        let token = std::env::var("NOTION_API_KEY")
            .ok()
            .or_else(|| std::env::var("NOTION_TOKEN").ok())?;
        if token.trim().is_empty() {
            return None;
        }
        let client = crate::client::shared_client().ok()?;
        Some(Self {
            client,
            token: token.trim().to_string(),
        })
    }

    async fn search(&self, query: &str, page_size: u32) -> Result<String, ToolError> {
        let body = json!({"query": query, "page_size": page_size.min(50)});
        self.post("/search", &body).await
    }

    async fn retrieve_page(&self, page_id: &str) -> Result<String, ToolError> {
        let path = format!("/pages/{}", percent_encode_path(page_id));
        self.get(&path).await
    }

    async fn append_text(&self, page_id: &str, text: &str) -> Result<String, ToolError> {
        let body = json!({
            "children": [{
                "object": "block",
                "type": "paragraph",
                "paragraph": {
                    "rich_text": [{
                        "type": "text",
                        "text": {"content": text}
                    }]
                }
            }]
        });
        let path = format!("/blocks/{}/children", percent_encode_path(page_id));
        self.patch(&path, &body).await
    }

    async fn get(&self, path: &str) -> Result<String, ToolError> {
        let url = format!("{NOTION_BASE}{path}");
        self.dispatch(self.client.get(url)).await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<String, ToolError> {
        let url = format!("{NOTION_BASE}{path}");
        self.dispatch(self.client.post(url).json(body)).await
    }

    async fn patch(&self, path: &str, body: &Value) -> Result<String, ToolError> {
        let url = format!("{NOTION_BASE}{path}");
        self.dispatch(self.client.patch(url).json(body)).await
    }

    async fn dispatch(&self, req: reqwest::RequestBuilder) -> Result<String, ToolError> {
        let resp = req
            .bearer_auth(&self.token)
            .header("Notion-Version", NOTION_API_VERSION)
            .send()
            .await
            .map_err(|e| ToolError::runtime(format!("notion request failed: {e}")))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ToolError::runtime(format!(
                "notion HTTP {status}: {}",
                truncate(&text, 500)
            )));
        }
        Ok(text)
    }
}

#[async_trait]
impl Tool for NotionTool {
    fn name(&self) -> &str {
        "notion"
    }

    fn description(&self) -> &str {
        "Search, read, and append to Notion pages. Supports actions: `search` (by text), \
         `retrieve_page` (by page id), and `append_text` (append a paragraph block to a page)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "retrieve_page", "append_text"],
                    "description": "Which Notion operation to perform."
                },
                "query": {"type": "string", "description": "Search query (for action=search)."},
                "page_id": {"type": "string", "description": "Notion page id (32-char UUID, dashes optional)."},
                "text": {"type": "string", "description": "Text to append as a paragraph (for action=append_text)."},
                "page_size": {"type": "integer", "description": "Max search results (1-50).", "minimum": 1, "maximum": 50}
            },
            "required": ["action"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::runtime("missing required 'action'"))?;
        match action {
            "search" => {
                let q = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let ps = args.get("page_size").and_then(|v| v.as_u64()).unwrap_or(10) as u32;
                self.search(&q, ps).await
            }
            "retrieve_page" => {
                let id = args
                    .get("page_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'page_id'"))?;
                self.retrieve_page(id).await
            }
            "append_text" => {
                let id = args
                    .get("page_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'page_id'"))?;
                let t = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'text'"))?;
                self.append_text(id, t).await
            }
            other => Err(ToolError::runtime(format!(
                "unknown notion action '{other}' — valid: search, retrieve_page, append_text"
            ))),
        }
    }
}

/// Percent-encode a path segment, allowing unreserved characters + hyphens
/// (Notion page IDs are UUIDs, sometimes with dashes).
fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_required_action() {
        let client = reqwest::Client::new();
        let t = NotionTool {
            client,
            token: "x".to_string(),
        };
        let s = t.parameters_schema();
        assert_eq!(s["required"][0], "action");
        let actions = s["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), 3);
    }

    #[tokio::test]
    async fn unknown_action_errors() {
        let client = reqwest::Client::new();
        let t = NotionTool {
            client,
            token: "x".to_string(),
        };
        let err = t.call(json!({"action": "delete"})).await.unwrap_err();
        assert!(err.to_string().contains("unknown notion action"));
    }

    #[tokio::test]
    async fn missing_action_errors() {
        let client = reqwest::Client::new();
        let t = NotionTool {
            client,
            token: "x".to_string(),
        };
        let err = t.call(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("action"));
    }

    #[test]
    #[serial_test::serial]
    fn from_env_needs_token() {
        std::env::remove_var("NOTION_API_KEY");
        std::env::remove_var("NOTION_TOKEN");
        assert!(NotionTool::from_env().is_none());
        std::env::set_var("NOTION_API_KEY", "secret-1");
        let t = NotionTool::from_env().unwrap();
        assert_eq!(t.token, "secret-1");
        std::env::remove_var("NOTION_API_KEY");
    }

    #[test]
    fn percent_encode_basic() {
        assert_eq!(percent_encode_path("abc-123"), "abc-123");
        assert_eq!(percent_encode_path("a b"), "a%20b");
    }
}

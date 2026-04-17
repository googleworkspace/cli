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

//! Browser-control tool (feature-gated on `browser-agent`).
//!
//! When the `browser-agent` feature is enabled (and a local Chrome/Chromium
//! binary is on PATH), this tool uses `headless_chrome` to drive a real
//! browser: navigate, click selectors, read text, and screenshot. When the
//! feature is disabled we still register a stub so the model receives an
//! informative error instead of an unexplained "no such tool" refusal.
//!
//! The browser instance is reused across tool calls within a single agent
//! session.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::tool::{Tool, ToolError};

#[cfg(feature = "browser-agent")]
mod backend {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    pub struct BrowserBackend {
        // `headless_chrome::Browser` is Send+Sync but not clonable; we wrap
        // it behind a Mutex so concurrent tool calls serialise.
        inner: Arc<Mutex<Option<headless_chrome::Browser>>>,
    }

    impl BrowserBackend {
        pub fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(None)),
            }
        }

        async fn browser(&self) -> Result<headless_chrome::Browser, ToolError> {
            let mut guard = self.inner.lock().await;
            if guard.is_none() {
                let b = tokio::task::spawn_blocking(headless_chrome::Browser::default)
                    .await
                    .map_err(|e| ToolError::runtime(format!("browser spawn: {e}")))?
                    .map_err(|e| ToolError::runtime(format!("browser launch: {e}")))?;
                *guard = Some(b);
            }
            Ok(guard.as_ref().unwrap().clone())
        }

        pub async fn run(&self, action: &str, args: &Value) -> Result<String, ToolError> {
            let browser = self.browser().await?;
            let args = args.clone();
            let action = action.to_string();
            tokio::task::spawn_blocking(move || run_blocking(&browser, &action, &args))
                .await
                .map_err(|e| ToolError::runtime(format!("browser task: {e}")))?
        }
    }

    fn run_blocking(
        browser: &headless_chrome::Browser,
        action: &str,
        args: &Value,
    ) -> Result<String, ToolError> {
        let tab = browser
            .new_tab()
            .map_err(|e| ToolError::runtime(format!("new_tab: {e}")))?;
        match action {
            "navigate" => {
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'url'"))?;
                tab.navigate_to(url)
                    .map_err(|e| ToolError::runtime(format!("navigate: {e}")))?;
                tab.wait_until_navigated()
                    .map_err(|e| ToolError::runtime(format!("wait: {e}")))?;
                Ok(format!("navigated to {url}"))
            }
            "click" => {
                let url = args.get("url").and_then(|v| v.as_str());
                let selector = args
                    .get("selector")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'selector'"))?;
                if let Some(u) = url {
                    tab.navigate_to(u)
                        .map_err(|e| ToolError::runtime(format!("navigate: {e}")))?;
                    tab.wait_until_navigated()
                        .map_err(|e| ToolError::runtime(format!("wait: {e}")))?;
                }
                let el = tab
                    .wait_for_element(selector)
                    .map_err(|e| ToolError::runtime(format!("element: {e}")))?;
                el.click()
                    .map_err(|e| ToolError::runtime(format!("click: {e}")))?;
                Ok(format!("clicked {selector}"))
            }
            "text" => {
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'url'"))?;
                tab.navigate_to(url)
                    .map_err(|e| ToolError::runtime(format!("navigate: {e}")))?;
                tab.wait_until_navigated()
                    .map_err(|e| ToolError::runtime(format!("wait: {e}")))?;
                let body = tab
                    .wait_for_element("body")
                    .map_err(|e| ToolError::runtime(format!("body: {e}")))?;
                let text = body
                    .get_inner_text()
                    .map_err(|e| ToolError::runtime(format!("inner text: {e}")))?;
                Ok(truncate(&text, 4000))
            }
            other => Err(ToolError::runtime(format!(
                "unknown browser action '{other}'"
            ))),
        }
    }

    fn truncate(s: &str, max: usize) -> String {
        if s.len() <= max {
            s.to_string()
        } else {
            format!("{}…(truncated {} bytes)", &s[..max], s.len() - max)
        }
    }
}

#[cfg(not(feature = "browser-agent"))]
mod backend {
    use super::*;

    pub struct BrowserBackend;

    impl BrowserBackend {
        pub fn new() -> Self {
            Self
        }

        pub async fn run(&self, _action: &str, _args: &Value) -> Result<String, ToolError> {
            Err(ToolError::runtime(
                "browser tool is not available: rebuild `gws` with --features browser-agent and \
                 ensure a Chrome/Chromium binary is on PATH",
            ))
        }
    }
}

pub struct BrowserTool {
    backend: backend::BrowserBackend,
}

impl BrowserTool {
    pub fn new() -> Self {
        Self {
            backend: backend::BrowserBackend::new(),
        }
    }
}

impl Default for BrowserTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Drive a headless browser. Actions: `navigate` (open a URL), `click` \
         (optionally navigate, then click a CSS selector), `text` (navigate \
         and return the page body text)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["navigate", "click", "text"]},
                "url": {"type": "string", "description": "Target URL."},
                "selector": {"type": "string", "description": "CSS selector (for action=click)."}
            },
            "required": ["action"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::runtime("missing 'action'"))?
            .to_string();
        self.backend.run(&action, &args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_action() {
        let t = BrowserTool::new();
        let s = t.parameters_schema();
        assert_eq!(s["required"][0], "action");
    }

    #[tokio::test]
    async fn missing_action_errors() {
        let t = BrowserTool::new();
        let err = t.call(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("action"));
    }

    // When the `browser-agent` feature is disabled (default in CI), the
    // stub backend must return an informative error instead of silently
    // succeeding.
    #[cfg(not(feature = "browser-agent"))]
    #[tokio::test]
    async fn stub_is_helpful() {
        let t = BrowserTool::new();
        let err = t
            .call(json!({"action": "navigate", "url": "https://example.com"}))
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("browser-agent"));
    }
}

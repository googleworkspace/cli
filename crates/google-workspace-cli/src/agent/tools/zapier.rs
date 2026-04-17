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

//! Zapier integration. Two tools:
//!
//! * [`ZapierWebhookTool`] — `POST`s JSON payloads to a "Catch Hook" Zap.
//!   Registered when `ZAPIER_WEBHOOK_URL` is set; additional named hooks
//!   can be configured by setting `ZAPIER_WEBHOOK_<NAME>_URL`.
//!
//! * [`ZapierNlaTool`] — Natural-Language Actions exposed through Zapier's
//!   AI Actions API. Registered when `ZAPIER_NLA_API_KEY` is set.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::agent::tool::{Tool, ToolError};

const ZAPIER_NLA_BASE: &str = "https://actions.zapier.com/api/v1";

pub struct ZapierWebhookTool {
    client: reqwest::Client,
    /// Map hook name → URL. The primary hook is under `default`.
    hooks: BTreeMap<String, String>,
}

impl ZapierWebhookTool {
    pub fn from_env() -> Option<Self> {
        let mut hooks = BTreeMap::new();
        if let Ok(v) = std::env::var("ZAPIER_WEBHOOK_URL") {
            let v = v.trim().to_string();
            if !v.is_empty() {
                hooks.insert("default".to_string(), v);
            }
        }
        // Discover ZAPIER_WEBHOOK_<NAME>_URL variants.
        for (k, v) in std::env::vars() {
            if let Some(rest) = k.strip_prefix("ZAPIER_WEBHOOK_") {
                if let Some(name) = rest.strip_suffix("_URL") {
                    let name = name.to_ascii_lowercase();
                    if name == "default" {
                        continue;
                    }
                    let v = v.trim().to_string();
                    if !v.is_empty() {
                        hooks.insert(name, v);
                    }
                }
            }
        }
        if hooks.is_empty() {
            return None;
        }
        let client = crate::client::shared_client().ok()?;
        Some(Self { client, hooks })
    }
}

#[async_trait]
impl Tool for ZapierWebhookTool {
    fn name(&self) -> &str {
        "zapier_webhook"
    }

    fn description(&self) -> &str {
        "Trigger a Zapier 'Catch Hook' by POSTing a JSON payload. Use `hook` to \
         select a named webhook (defaults to 'default'); put the payload in `data`."
    }

    fn parameters_schema(&self) -> Value {
        let names: Vec<&str> = self.hooks.keys().map(|s| s.as_str()).collect();
        json!({
            "type": "object",
            "properties": {
                "hook": {
                    "type": "string",
                    "description": "Named hook to invoke.",
                    "enum": names,
                    "default": "default"
                },
                "data": {
                    "type": "object",
                    "description": "Arbitrary JSON payload forwarded to Zapier.",
                    "additionalProperties": true
                }
            },
            "required": ["data"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, ToolError> {
        let hook = args
            .get("hook")
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        let url = self.hooks.get(hook).ok_or_else(|| {
            ToolError::runtime(format!(
                "unknown hook '{hook}' — available: {}",
                self.hooks.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })?;
        let data = args
            .get("data")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        let resp = self
            .client
            .post(url)
            .json(&data)
            .send()
            .await
            .map_err(|e| ToolError::runtime(format!("zapier webhook: {e}")))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ToolError::runtime(format!(
                "zapier webhook HTTP {status}: {text}"
            )));
        }
        Ok(format!("status={status} body={text}"))
    }
}

pub struct ZapierNlaTool {
    client: reqwest::Client,
    api_key: String,
}

impl ZapierNlaTool {
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("ZAPIER_NLA_API_KEY").ok()?;
        if key.trim().is_empty() {
            return None;
        }
        let client = crate::client::shared_client().ok()?;
        Some(Self {
            client,
            api_key: key.trim().to_string(),
        })
    }
}

#[async_trait]
impl Tool for ZapierNlaTool {
    fn name(&self) -> &str {
        "zapier_actions"
    }

    fn description(&self) -> &str {
        "Invoke Zapier Natural-Language Actions. Use action='list' to discover \
         exposed actions, or action='run' with action_id plus a plain-English \
         instruction."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "run"]},
                "action_id": {"type": "string", "description": "Zapier exposed action id (for action=run)."},
                "instructions": {"type": "string", "description": "Natural-language instructions describing what the action should do."},
                "params": {"type": "object", "description": "Optional structured overrides."}
            },
            "required": ["action"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::runtime("missing 'action'"))?;
        match action {
            "list" => {
                let resp = self
                    .client
                    .get(format!("{ZAPIER_NLA_BASE}/exposed/"))
                    .header("x-api-key", &self.api_key)
                    .send()
                    .await
                    .map_err(|e| ToolError::runtime(format!("zapier list: {e}")))?;
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if !status.is_success() {
                    return Err(ToolError::runtime(format!(
                        "zapier list HTTP {status}: {text}"
                    )));
                }
                Ok(text)
            }
            "run" => {
                let id = args
                    .get("action_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'action_id'"))?;
                let instructions = args
                    .get("instructions")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let mut body = json!({"instructions": instructions});
                if let Some(params) = args.get("params") {
                    body["params"] = params.clone();
                }
                let resp = self
                    .client
                    .post(format!("{ZAPIER_NLA_BASE}/exposed/{id}/execute/"))
                    .header("x-api-key", &self.api_key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| ToolError::runtime(format!("zapier run: {e}")))?;
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if !status.is_success() {
                    return Err(ToolError::runtime(format!(
                        "zapier run HTTP {status}: {text}"
                    )));
                }
                Ok(text)
            }
            other => Err(ToolError::runtime(format!(
                "unknown zapier action '{other}'"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn webhook_from_env_requires_any_url() {
        std::env::remove_var("ZAPIER_WEBHOOK_URL");
        // Clear any test-local custom hooks.
        let to_clear: Vec<String> = std::env::vars()
            .map(|(k, _)| k)
            .filter(|k| k.starts_with("ZAPIER_WEBHOOK_"))
            .collect();
        for k in &to_clear {
            std::env::remove_var(k);
        }
        assert!(ZapierWebhookTool::from_env().is_none());

        std::env::set_var(
            "ZAPIER_WEBHOOK_URL",
            "https://hooks.zapier.com/hooks/catch/1/abc/",
        );
        std::env::set_var("ZAPIER_WEBHOOK_REMINDERS_URL", "https://hooks.zapier.com/x");
        let t = ZapierWebhookTool::from_env().unwrap();
        assert!(t.hooks.contains_key("default"));
        assert!(t.hooks.contains_key("reminders"));
        std::env::remove_var("ZAPIER_WEBHOOK_URL");
        std::env::remove_var("ZAPIER_WEBHOOK_REMINDERS_URL");
    }

    #[tokio::test]
    async fn webhook_rejects_unknown_hook() {
        let client = reqwest::Client::new();
        let mut hooks = BTreeMap::new();
        hooks.insert("default".to_string(), "https://example.test".to_string());
        let tool = ZapierWebhookTool { client, hooks };
        let err = tool
            .call(json!({"hook": "ghost", "data": {}}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown hook"));
    }

    #[tokio::test]
    async fn nla_rejects_unknown_action() {
        let tool = ZapierNlaTool {
            client: reqwest::Client::new(),
            api_key: "x".to_string(),
        };
        let err = tool.call(json!({"action": "bogus"})).await.unwrap_err();
        assert!(err.to_string().contains("unknown zapier action"));
    }

    #[tokio::test]
    async fn nla_run_requires_action_id() {
        let tool = ZapierNlaTool {
            client: reqwest::Client::new(),
            api_key: "x".to_string(),
        };
        let err = tool.call(json!({"action": "run"})).await.unwrap_err();
        assert!(err.to_string().contains("action_id"));
    }
}

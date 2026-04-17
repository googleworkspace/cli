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

//! OpenAI-compatible chat/completions client.
//!
//! Both OpenRouter and Ollama (via its built-in OpenAI compatibility layer
//! at `/v1/chat/completions`) accept this request shape, so a single client
//! serves both providers. The only difference is the `Authorization` header
//! and the attribution headers OpenRouter uses for usage tracking.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::config::{AgentConfig, ProviderKind};
use crate::error::GwsError;

/// Role of a single chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// One chat-completions message. Optional fields match OpenAI semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Assistant-initiated tool calls (populated on assistant messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// For tool role: id of the call this result belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For tool role: function name that was invoked (optional per spec).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn system<S: Into<String>>(text: S) -> Self {
        Self {
            role: Role::System,
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }
    pub fn user<S: Into<String>>(text: S) -> Self {
        Self {
            role: Role::User,
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }
    pub fn assistant_text<S: Into<String>>(text: S) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }
    pub fn tool_result<S1: Into<String>, S2: Into<String>, S3: Into<String>>(
        call_id: S1,
        name: S2,
        content: S3,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
            name: Some(name.into()),
        }
    }
}

/// Assistant-issued tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    /// OpenAI uses `"function"` as the discriminator.
    #[serde(rename = "type", default = "default_tool_type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

fn default_tool_type() -> String {
    "function".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON-encoded arguments. Always a string per the OpenAI spec, even
    /// though providers sometimes return an object — we normalise below.
    pub arguments: String,
}

/// Strongly-typed LLM response suitable for driving the agent loop.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    /// Provider-reported finish reason. Surfaced for future telemetry /
    /// logging hooks; not currently consulted by the agent loop.
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[Value],
    ) -> Result<LlmResponse, GwsError>;

    #[allow(dead_code)]
    fn provider(&self) -> ProviderKind;
    #[allow(dead_code)]
    fn model(&self) -> &str;
}

/// Shared OpenAI-compatible client.
pub struct OpenAiCompatibleClient {
    client: reqwest::Client,
    config: AgentConfig,
}

impl OpenAiCompatibleClient {
    pub fn new(config: AgentConfig) -> Result<Self, GwsError> {
        let client = crate::client::shared_client()
            .map_err(|e| GwsError::Other(anyhow::anyhow!("agent http client: {e}")))?;
        Ok(Self { client, config })
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

#[derive(Debug, Deserialize)]
struct RawChoice {
    message: RawMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    tool_calls: Option<Vec<RawToolCall>>,
}

#[derive(Debug, Deserialize)]
struct RawToolCall {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    call_type: Option<String>,
    function: RawToolCallFunction,
}

#[derive(Debug, Deserialize)]
struct RawToolCallFunction {
    name: String,
    /// Some providers (Ollama) emit a JSON object here instead of a string;
    /// accept either and normalise to a string downstream.
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct RawResponse {
    choices: Vec<RawChoice>,
}

/// Flatten content that may come back as either a string or the
/// OpenAI-style list of content parts.
fn content_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            let joined: String = parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("");
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        }
        other => Some(other.to_string()),
    }
}

fn arguments_to_string(v: &Value) -> String {
    match v {
        Value::Null => "{}".to_string(),
        Value::String(s) if s.trim().is_empty() => "{}".to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleClient {
    async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[Value],
    ) -> Result<LlmResponse, GwsError> {
        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
            body["tool_choice"] = Value::String("auto".to_string());
        }
        if let Some(t) = self.config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(m) = self.config.max_tokens {
            body["max_tokens"] = serde_json::json!(m);
        }

        let mut req = self.client.post(self.endpoint()).json(&body);
        if let Some(ref key) = self.config.api_key {
            req = req.bearer_auth(key);
        }
        if self.config.provider == ProviderKind::OpenRouter {
            req = req
                .header("HTTP-Referer", &self.config.app_referer)
                .header("X-Title", &self.config.app_title);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("LLM request failed: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("LLM response read failed: {e}")))?;
        if !status.is_success() {
            return Err(GwsError::Other(anyhow::anyhow!(
                "LLM HTTP {status}: {}",
                truncate(&text, 500)
            )));
        }
        let raw: RawResponse = serde_json::from_str(&text).map_err(|e| {
            GwsError::Other(anyhow::anyhow!(
                "LLM JSON parse failed: {e}; body: {}",
                truncate(&text, 300)
            ))
        })?;

        let choice = raw
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| GwsError::Other(anyhow::anyhow!("LLM returned no choices")))?;

        let content = choice.message.content.as_ref().and_then(content_to_string);
        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, t)| ToolCall {
                id: t.id.unwrap_or_else(|| format!("call_{i}")),
                call_type: t.call_type.unwrap_or_else(|| "function".to_string()),
                function: ToolCallFunction {
                    name: t.function.name,
                    arguments: arguments_to_string(&t.function.arguments),
                },
            })
            .collect();

        Ok(LlmResponse {
            content,
            tool_calls,
            finish_reason: choice.finish_reason,
        })
    }

    fn provider(&self) -> ProviderKind {
        self.config.provider
    }

    fn model(&self) -> &str {
        &self.config.model
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…(truncated {} bytes)", &s[..max], s.len() - max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn role_as_str_roundtrip() {
        assert_eq!(Role::System.as_str(), "system");
        assert_eq!(Role::Tool.as_str(), "tool");
    }

    #[test]
    fn chat_message_constructors_omit_empty_fields_in_json() {
        let msg = ChatMessage::user("hi");
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["role"], "user");
        assert_eq!(v["content"], "hi");
        assert!(v.get("tool_calls").is_none());
        assert!(v.get("name").is_none());
    }

    #[test]
    fn tool_result_shape() {
        let m = ChatMessage::tool_result("abc", "notion_search", "[]");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["tool_call_id"], "abc");
        assert_eq!(v["name"], "notion_search");
        assert_eq!(v["content"], "[]");
    }

    #[test]
    fn content_string_variants() {
        assert_eq!(content_to_string(&json!("hi")).as_deref(), Some("hi"));
        assert_eq!(content_to_string(&json!("")), None);
        assert_eq!(content_to_string(&Value::Null), None);
        assert_eq!(
            content_to_string(&json!([{"type":"text","text":"a"},{"type":"text","text":"b"}]))
                .as_deref(),
            Some("ab")
        );
    }

    #[test]
    fn arguments_to_string_handles_object_or_string() {
        assert_eq!(arguments_to_string(&json!("{\"x\":1}")), "{\"x\":1}");
        assert_eq!(arguments_to_string(&json!({"x":1})), "{\"x\":1}");
        assert_eq!(arguments_to_string(&Value::Null), "{}");
        assert_eq!(arguments_to_string(&json!("  ")), "{}");
    }

    #[test]
    fn truncate_short_and_long() {
        assert_eq!(truncate("abc", 10), "abc");
        let s = truncate("abcdef", 3);
        assert!(s.starts_with("abc"));
        assert!(s.contains("truncated"));
    }

    #[test]
    fn raw_response_parses_openai_shape() {
        let body = json!({
            "id": "x",
            "choices": [
                {
                    "finish_reason": "tool_calls",
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {"name": "notion_search", "arguments": "{\"q\":\"r\"}"}
                            }
                        ]
                    }
                }
            ]
        });
        let raw: RawResponse = serde_json::from_value(body).unwrap();
        let c = &raw.choices[0];
        assert_eq!(c.finish_reason.as_deref(), Some("tool_calls"));
        let tc = c.message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "notion_search");
    }

    #[test]
    fn raw_response_parses_ollama_object_arguments() {
        let body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "function": {"name": "f", "arguments": {"a": 1}}
                    }]
                }
            }]
        });
        let raw: RawResponse = serde_json::from_value(body).unwrap();
        let args = &raw.choices[0].message.tool_calls.as_ref().unwrap()[0]
            .function
            .arguments;
        assert_eq!(arguments_to_string(args), "{\"a\":1}");
    }
}

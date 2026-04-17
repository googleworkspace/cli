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

//! Tool abstraction for the agent loop.
//!
//! A `Tool` is anything the LLM can invoke through an OpenAI-style
//! `tool_calls` function. Each tool advertises a name, a natural-language
//! description and a JSON Schema describing its arguments; execution takes
//! a `serde_json::Value` of arguments and returns a `String` that is fed
//! back to the model as the `tool` role message.
//!
//! Tools are expected to be cheap to clone (typically they hold
//! `Arc`-wrapped HTTP clients + configuration).

use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Error returned from tool execution.
///
/// We never propagate raw network or deserialization errors to the model —
/// tool failures are serialized into the tool-call response so the model
/// can recover.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// User declined an interactive approval prompt.
    #[error("user denied tool execution")]
    Denied,
    /// Any runtime failure (network, parse, remote API error) — already
    /// formatted for presentation to the model.
    #[error("{0}")]
    Runtime(String),
}

impl ToolError {
    pub fn runtime<M: Into<String>>(msg: M) -> Self {
        ToolError::Runtime(msg.into())
    }
}

/// A single tool the agent can invoke.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique snake_case identifier sent to the model.
    fn name(&self) -> &str;
    /// Short human description shown to the model.
    fn description(&self) -> &str;
    /// JSON Schema (draft-07 style) for the arguments object.
    fn parameters_schema(&self) -> Value;
    /// Execute the tool and return a string result.
    async fn call(&self, args: Value) -> Result<String, ToolError>;
}

/// OpenAI-compatible tool specification shipped to the model.
pub fn openai_tool_spec(tool: &dyn Tool) -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name(),
            "description": tool.description(),
            "parameters": tool.parameters_schema(),
        }
    })
}

/// Collection of tools indexed by name.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Names in insertion / alphabetical order (BTreeMap preserves order).
    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Retain only tools whose names appear in `allow`. An empty allow-list
    /// means "keep everything".
    pub fn retain(&mut self, allow: &[String]) {
        if allow.is_empty() {
            return;
        }
        let allow: std::collections::HashSet<&str> = allow.iter().map(|s| s.as_str()).collect();
        self.tools.retain(|k, _| allow.contains(k.as_str()));
    }

    /// OpenAI-style tool specs for every registered tool.
    pub fn as_openai_specs(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| openai_tool_spec(t.as_ref()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Echo;
    #[async_trait]
    impl Tool for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echo the input string back."
        }
        fn parameters_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"],
            })
        }
        async fn call(&self, args: Value) -> Result<String, ToolError> {
            let t = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::runtime("missing 'text'"))?;
            Ok(t.to_string())
        }
    }

    #[test]
    fn registry_retains_allowlist() {
        let mut r = ToolRegistry::new();
        r.insert(Arc::new(Echo));
        r.insert(Arc::new(RenamedEcho("alpha")));
        r.insert(Arc::new(RenamedEcho("beta")));
        assert_eq!(r.len(), 3);
        r.retain(&["alpha".to_string(), "echo".to_string()]);
        assert_eq!(r.len(), 2);
        assert!(r.get("alpha").is_some());
        assert!(r.get("echo").is_some());
        assert!(r.get("beta").is_none());
    }

    #[test]
    fn empty_retain_is_noop() {
        let mut r = ToolRegistry::new();
        r.insert(Arc::new(Echo));
        r.retain(&[]);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn openai_spec_shape() {
        let spec = openai_tool_spec(&Echo);
        assert_eq!(spec["type"], "function");
        assert_eq!(spec["function"]["name"], "echo");
        assert!(spec["function"]["parameters"]["properties"]["text"].is_object());
    }

    #[tokio::test]
    async fn echo_tool_runs() {
        let tool = Echo;
        let out = tool.call(json!({"text": "hi"})).await.unwrap();
        assert_eq!(out, "hi");
    }

    struct RenamedEcho(&'static str);
    #[async_trait]
    impl Tool for RenamedEcho {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            "renamed echo"
        }
        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }
        async fn call(&self, _args: Value) -> Result<String, ToolError> {
            Ok(self.0.to_string())
        }
    }
}

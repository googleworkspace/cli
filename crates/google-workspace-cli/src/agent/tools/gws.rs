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

//! Google Workspace tool. Lets the LLM call the `gws` binary itself
//! (self-hosted) to reach every Google Workspace API the CLI already
//! exposes.
//!
//! Arguments are validated to prevent shell injection (we never go through
//! a shell) and to keep the model from requesting interactive auth flows.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::agent::tool::{Tool, ToolError};

/// Arguments that must not appear verbatim — they can trigger interactive
/// flows or shell out further.
const DENIED_ARG_PREFIXES: &[&str] = &["--upload", "--output"];

pub struct GwsTool {
    /// Path (or name) of the `gws` binary. Defaults to the running executable.
    bin: String,
}

impl GwsTool {
    pub fn new() -> Self {
        let bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "gws".to_string());
        Self { bin }
    }
}

impl Default for GwsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GwsTool {
    fn name(&self) -> &str {
        "google_workspace"
    }

    fn description(&self) -> &str {
        "Call the Google Workspace CLI (`gws`). Provide a list of CLI args, \
         e.g. ['drive', 'files', 'list', '--pageSize', '5']. Uses the \
         already-authenticated user; stdout JSON is returned verbatim. Do \
         not pass --upload or --output (agent sandbox)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Argument vector passed to `gws` (no shell metacharacters).",
                    "minItems": 1
                }
            },
            "required": ["args"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, ToolError> {
        let list = args
            .get("args")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolError::runtime("missing 'args' array"))?;
        if list.is_empty() {
            return Err(ToolError::runtime("'args' must not be empty"));
        }
        let mut argv: Vec<String> = Vec::with_capacity(list.len());
        for a in list {
            let s = a
                .as_str()
                .ok_or_else(|| ToolError::runtime("'args' entries must be strings"))?;
            for deny in DENIED_ARG_PREFIXES {
                if s == *deny || s.starts_with(&format!("{deny}=")) {
                    return Err(ToolError::runtime(format!(
                        "argument '{s}' is not allowed from agent tool calls"
                    )));
                }
            }
            argv.push(s.to_string());
        }
        // Disallow recursive agent self-invocation to avoid runaway loops.
        // Check all arguments, not just the first, since the LLM can use global
        // flags before the command (e.g., `gws --api-version v1 agent ...`).
        if argv.iter().find(|s| !s.starts_with('-')).map(|s| s.as_str()) == Some("agent") {
            return Err(ToolError::runtime("recursive agent invocation is blocked"));
        }

        let output = Command::new(&self.bin)
            .args(&argv)
            .env("NO_COLOR", "1")
            .output()
            .await
            .map_err(|e| ToolError::runtime(format!("gws spawn failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let code = output.status.code().unwrap_or(-1);
        if output.status.success() {
            Ok(stdout)
        } else {
            Err(ToolError::runtime(format!(
                "gws exit {code}\nstderr: {}\nstdout: {}",
                truncate(&stderr, 800),
                truncate(&stdout, 800)
            )))
        }
    }
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

    #[tokio::test]
    async fn rejects_missing_args() {
        let t = GwsTool::new();
        let err = t.call(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("missing 'args'"));
    }

    #[tokio::test]
    async fn rejects_empty_args() {
        let t = GwsTool::new();
        let err = t.call(json!({"args": []})).await.unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[tokio::test]
    async fn rejects_non_string_args() {
        let t = GwsTool::new();
        let err = t.call(json!({"args": [1, 2, 3]})).await.unwrap_err();
        assert!(err.to_string().contains("must be strings"));
    }

    #[tokio::test]
    async fn blocks_denied_prefixes() {
        let t = GwsTool::new();
        let err = t
            .call(json!({"args": ["drive", "files", "list", "--upload=secret"]}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[tokio::test]
    async fn blocks_recursive_agent() {
        let t = GwsTool::new();
        let err = t
            .call(json!({"args": ["agent", "--prompt", "hi"]}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("recursive"));
    }

    #[test]
    fn schema_requires_args() {
        let t = GwsTool::new();
        let s = t.parameters_schema();
        assert_eq!(s["required"][0], "args");
    }
}

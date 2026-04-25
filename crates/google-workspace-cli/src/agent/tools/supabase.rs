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

//! Supabase (PostgREST) tool. Exposes `select`, `insert`, `update`, and
//! `delete` against arbitrary tables, plus an `rpc` entry for calling
//! Postgres functions.
//!
//! The caller must supply a table name; we validate it against a strict
//! identifier pattern (`[a-zA-Z_][a-zA-Z0-9_]*`) to prevent URL injection
//! before embedding it into the REST path.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::agent::config::AgentConfig;
use crate::agent::tool::{Tool, ToolError};

pub struct SupabaseTool {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl SupabaseTool {
    pub fn from_config(config: &AgentConfig) -> Option<Self> {
        let base = config
            .supabase_url
            .as_ref()?
            .trim_end_matches('/')
            .to_string();
        let key = config.supabase_key.as_ref()?.clone();
        if base.is_empty() || key.is_empty() {
            return None;
        }
        let client = crate::client::shared_client().ok()?;
        Some(Self {
            client,
            base_url: base,
            api_key: key,
        })
    }

    fn rest_url(&self, table: &str) -> String {
        format!("{}/rest/v1/{}", self.base_url, table)
    }

    fn rpc_url(&self, func: &str) -> String {
        format!("{}/rest/v1/rpc/{}", self.base_url, func)
    }

    async fn handle_resp(resp: reqwest::Response) -> Result<String, ToolError> {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ToolError::runtime(format!(
                "supabase HTTP {status}: {text}"
            )));
        }
        Ok(text)
    }
}

/// Validate that a string is safe to embed in a PostgREST URL as a table
/// or column identifier.
fn validate_identifier(s: &str, label: &str) -> Result<(), ToolError> {
    if s.is_empty() {
        return Err(ToolError::runtime(format!("{label} must not be empty")));
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(ToolError::runtime(format!(
            "{label} must start with an ASCII letter or underscore"
        )));
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return Err(ToolError::runtime(format!(
                "{label} may only contain ASCII alphanumerics or underscores"
            )));
        }
    }
    Ok(())
}

/// Build a PostgREST query string from a `filter` object of the form
/// `{"column": {"op": "eq", "value": "x"}}`. Falsy `op` defaults to `eq`.
fn build_query(
    filter: Option<&Value>,
    limit: Option<u64>,
    order: Option<&str>,
) -> Result<String, ToolError> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(Value::Object(map)) = filter {
        for (col, spec) in map {
            validate_identifier(col, "filter column")?;
            let (op, value) = match spec {
                Value::Object(m) => {
                    let op = m
                        .get("op")
                        .and_then(|v| v.as_str())
                        .unwrap_or("eq")
                        .to_string();
                    let v = m.get("value").cloned().unwrap_or(Value::Null);
                    (op, v)
                }
                other => ("eq".to_string(), other.clone()),
            };
            if !matches!(
                op.as_str(),
                "eq" | "neq" | "gt" | "gte" | "lt" | "lte" | "like" | "ilike" | "is" | "in"
            ) {
                return Err(ToolError::runtime(format!(
                    "unsupported filter operator '{op}'"
                )));
            }
            let value_str = match &value {
                Value::String(s) => s.clone(),
                Value::Null => "null".to_string(),
                other => other.to_string(),
            };
            parts.push(format!("{col}={op}.{}", encode(&value_str)));
        }
    }
    if let Some(n) = limit {
        parts.push(format!("limit={n}"));
    }
    if let Some(o) = order {
        // Simple allow-list: `col` or `col.asc` / `col.desc`
        let mut split = o.splitn(2, '.');
        let col = split.next().unwrap_or("");
        validate_identifier(col, "order column")?;
        if let Some(dir) = split.next() {
            if !matches!(dir, "asc" | "desc") {
                return Err(ToolError::runtime(format!(
                    "unsupported order direction '{dir}'"
                )));
            }
        }
        parts.push(format!("order={}", encode(o)));
    }
    Ok(parts.join("&"))
}

fn encode(s: &str) -> String {
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

#[async_trait]
impl Tool for SupabaseTool {
    fn name(&self) -> &str {
        "supabase"
    }

    fn description(&self) -> &str {
        "Read and write Supabase (Postgres) rows via PostgREST. Actions: \
         `select`, `insert`, `update`, `delete`, `rpc`. Identifiers are \
         validated; filters use {op, value} pairs."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["select", "insert", "update", "delete", "rpc"]},
                "table": {"type": "string", "description": "Table name (identifier only)."},
                "function": {"type": "string", "description": "RPC function name (for action=rpc)."},
                "filter": {
                    "type": "object",
                    "description": "Column filters, e.g. {\"id\": {\"op\": \"eq\", \"value\": \"123\"}}.",
                    "additionalProperties": true
                },
                "row": {"type": "object", "description": "Row body for insert/update.", "additionalProperties": true},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000},
                "order": {"type": "string", "description": "Order clause like 'created_at.desc'."},
                "params": {"type": "object", "description": "Params for action=rpc.", "additionalProperties": true}
            },
            "required": ["action"]
        })
    }

    async fn call(&self, args: Value) -> Result<String, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::runtime("missing 'action'"))?;
        let filter = args.get("filter");
        let limit = args.get("limit").and_then(|v| v.as_u64());
        let order = args.get("order").and_then(|v| v.as_str());
        let query = build_query(filter, limit, order)?;

        match action {
            "select" => {
                let table = args
                    .get("table")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'table'"))?;
                validate_identifier(table, "table")?;
                let url = if query.is_empty() {
                    format!("{}?select=*", self.rest_url(table))
                } else {
                    format!("{}?select=*&{}", self.rest_url(table), query)
                };
                let resp = self
                    .client
                    .get(url)
                    .header("apikey", &self.api_key)
                    .bearer_auth(&self.api_key)
                    .send()
                    .await
                    .map_err(|e| ToolError::runtime(format!("supabase select: {e}")))?;
                Self::handle_resp(resp).await
            }
            "insert" => {
                let table = args
                    .get("table")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'table'"))?;
                validate_identifier(table, "table")?;
                let row = args
                    .get("row")
                    .cloned()
                    .ok_or_else(|| ToolError::runtime("missing 'row'"))?;
                let body = match row {
                    Value::Array(_) => row,
                    other => Value::Array(vec![other]),
                };
                let resp = self
                    .client
                    .post(self.rest_url(table))
                    .header("apikey", &self.api_key)
                    .bearer_auth(&self.api_key)
                    .header("Content-Type", "application/json")
                    .header("Prefer", "return=representation")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| ToolError::runtime(format!("supabase insert: {e}")))?;
                Self::handle_resp(resp).await
            }
            "update" => {
                let table = args
                    .get("table")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'table'"))?;
                validate_identifier(table, "table")?;
                if query.is_empty() {
                    return Err(ToolError::runtime(
                        "update requires a non-empty 'filter' (safety guard against full-table updates)",
                    ));
                }
                let row = args
                    .get("row")
                    .cloned()
                    .ok_or_else(|| ToolError::runtime("missing 'row'"))?;
                let url = format!("{}?{}", self.rest_url(table), query);
                let resp = self
                    .client
                    .patch(url)
                    .header("apikey", &self.api_key)
                    .bearer_auth(&self.api_key)
                    .header("Content-Type", "application/json")
                    .header("Prefer", "return=representation")
                    .json(&row)
                    .send()
                    .await
                    .map_err(|e| ToolError::runtime(format!("supabase update: {e}")))?;
                Self::handle_resp(resp).await
            }
            "delete" => {
                let table = args
                    .get("table")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'table'"))?;
                validate_identifier(table, "table")?;
                if query.is_empty() {
                    return Err(ToolError::runtime(
                        "delete requires a non-empty 'filter' (safety guard)",
                    ));
                }
                let url = format!("{}?{}", self.rest_url(table), query);
                let resp = self
                    .client
                    .delete(url)
                    .header("apikey", &self.api_key)
                    .bearer_auth(&self.api_key)
                    .send()
                    .await
                    .map_err(|e| ToolError::runtime(format!("supabase delete: {e}")))?;
                Self::handle_resp(resp).await
            }
            "rpc" => {
                let func = args
                    .get("function")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::runtime("missing 'function'"))?;
                validate_identifier(func, "function")?;
                let params = args
                    .get("params")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Default::default()));
                let resp = self
                    .client
                    .post(self.rpc_url(func))
                    .header("apikey", &self.api_key)
                    .bearer_auth(&self.api_key)
                    .json(&params)
                    .send()
                    .await
                    .map_err(|e| ToolError::runtime(format!("supabase rpc: {e}")))?;
                Self::handle_resp(resp).await
            }
            other => Err(ToolError::runtime(format!(
                "unknown supabase action '{other}'"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_accepts_plain() {
        assert!(validate_identifier("users", "table").is_ok());
        assert!(validate_identifier("_private_1", "x").is_ok());
    }

    #[test]
    fn identifier_rejects_injection() {
        assert!(validate_identifier("users;drop", "t").is_err());
        assert!(validate_identifier("users table", "t").is_err());
        assert!(validate_identifier("", "t").is_err());
        assert!(validate_identifier("1users", "t").is_err());
    }

    #[test]
    fn build_query_default_op_is_eq() {
        let q = build_query(Some(&json!({"id": "abc"})), None, None).unwrap();
        assert_eq!(q, "id=eq.abc");
    }

    #[test]
    fn build_query_with_op_and_limit_order() {
        let q = build_query(
            Some(&json!({"age": {"op": "gte", "value": 18}})),
            Some(25),
            Some("created_at.desc"),
        )
        .unwrap();
        assert!(q.contains("age=gte.18"));
        assert!(q.contains("limit=25"));
        assert!(q.contains("order=created_at.desc"));
    }

    #[test]
    fn build_query_rejects_bad_operator() {
        let err = build_query(
            Some(&json!({"x": {"op": "; drop", "value": 1}})),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsupported filter operator"));
    }

    #[test]
    fn build_query_rejects_bad_order_direction() {
        let err = build_query(None, None, Some("x.sideways")).unwrap_err();
        assert!(err.to_string().contains("order direction"));
    }

    #[tokio::test]
    async fn update_without_filter_is_refused() {
        let tool = SupabaseTool {
            client: reqwest::Client::new(),
            base_url: "https://example.test".to_string(),
            api_key: "x".to_string(),
        };
        let err = tool
            .call(json!({"action": "update", "table": "users", "row": {"a": 1}}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("non-empty 'filter'"));
    }

    #[tokio::test]
    async fn delete_without_filter_is_refused() {
        let tool = SupabaseTool {
            client: reqwest::Client::new(),
            base_url: "https://example.test".to_string(),
            api_key: "x".to_string(),
        };
        let err = tool
            .call(json!({"action": "delete", "table": "users"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("safety guard"));
    }

    #[tokio::test]
    async fn unknown_action_errors() {
        let tool = SupabaseTool {
            client: reqwest::Client::new(),
            base_url: "https://example.test".to_string(),
            api_key: "x".to_string(),
        };
        let err = tool.call(json!({"action": "truncate"})).await.unwrap_err();
        assert!(err.to_string().contains("unknown supabase action"));
    }
}

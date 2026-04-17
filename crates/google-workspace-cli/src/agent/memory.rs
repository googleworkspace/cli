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

//! LangChain-style conversational memory.
//!
//! The agent reads any prior `user` / `assistant` turns for the current
//! `conversation_id` on start-up, then writes back the new user turn and
//! the final assistant answer after the loop completes. Tool traffic is
//! *not* persisted — it is recomputed per-turn to avoid bloating context
//! and because tool results often contain freshness-sensitive data.
//!
//! Two backends are supported:
//! * [`BufferMemory`] — in-process only, used as a fallback when Supabase
//!   isn't configured and for tests.
//! * [`SupabaseMemory`] — persists to a Supabase table via the REST
//!   (PostgREST) API. The table schema is:
//!
//!   ```sql
//!   create table gws_agent_memory (
//!     id bigserial primary key,
//!     conversation_id text not null,
//!     role text not null,
//!     content text not null,
//!     created_at timestamptz not null default now()
//!   );
//!   create index on gws_agent_memory (conversation_id, created_at);
//!   ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::agent::config::AgentConfig;
use crate::agent::llm::{ChatMessage, Role};
use crate::error::GwsError;

/// A persistable conversation turn. Only `user` and `assistant` messages
/// survive round-trips — system prompts and tool messages are stripped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryTurn {
    pub role: String,
    pub content: String,
}

impl MemoryTurn {
    pub fn from_message(m: &ChatMessage) -> Option<Self> {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            _ => return None,
        };
        let content = m.content.clone()?;
        Some(Self {
            role: role.to_string(),
            content,
        })
    }

    pub fn to_message(&self) -> Option<ChatMessage> {
        match self.role.as_str() {
            "user" => Some(ChatMessage::user(&self.content)),
            "assistant" => Some(ChatMessage::assistant_text(&self.content)),
            _ => None,
        }
    }
}

#[async_trait]
pub trait Memory: Send + Sync {
    async fn load(&self, conversation_id: &str) -> Result<Vec<MemoryTurn>, GwsError>;
    async fn append(&self, conversation_id: &str, turn: &MemoryTurn) -> Result<(), GwsError>;
    fn kind(&self) -> &'static str;
}

/// In-process ring buffer (per conversation_id).
#[derive(Default)]
pub struct BufferMemory {
    inner: tokio::sync::Mutex<std::collections::HashMap<String, Vec<MemoryTurn>>>,
    capacity: usize,
}

impl BufferMemory {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            capacity,
        }
    }
}

#[async_trait]
impl Memory for BufferMemory {
    async fn load(&self, conversation_id: &str) -> Result<Vec<MemoryTurn>, GwsError> {
        let g = self.inner.lock().await;
        Ok(g.get(conversation_id).cloned().unwrap_or_default())
    }

    async fn append(&self, conversation_id: &str, turn: &MemoryTurn) -> Result<(), GwsError> {
        let mut g = self.inner.lock().await;
        let entry = g.entry(conversation_id.to_string()).or_default();
        entry.push(turn.clone());
        if self.capacity > 0 && entry.len() > self.capacity {
            let overflow = entry.len() - self.capacity;
            entry.drain(0..overflow);
        }
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "buffer"
    }
}

/// Supabase-backed memory using the PostgREST HTTP API.
pub struct SupabaseMemory {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    table: String,
}

impl SupabaseMemory {
    pub fn from_config(config: &AgentConfig) -> Result<Option<Self>, GwsError> {
        let (url, key) = match (&config.supabase_url, &config.supabase_key) {
            (Some(u), Some(k)) => (u.clone(), k.clone()),
            _ => return Ok(None),
        };
        let client = crate::client::shared_client()
            .map_err(|e| GwsError::Other(anyhow::anyhow!("supabase http client: {e}")))?;
        Ok(Some(Self {
            client,
            base_url: url.trim_end_matches('/').to_string(),
            api_key: key,
            table: config.supabase_memory_table.clone(),
        }))
    }

    fn endpoint(&self) -> String {
        format!("{}/rest/v1/{}", self.base_url, self.table)
    }
}

#[async_trait]
impl Memory for SupabaseMemory {
    async fn load(&self, conversation_id: &str) -> Result<Vec<MemoryTurn>, GwsError> {
        let url = format!(
            "{}?conversation_id=eq.{}&order=created_at.asc&select=role,content",
            self.endpoint(),
            urlencoding(conversation_id)
        );
        let resp = self
            .client
            .get(&url)
            .header("apikey", &self.api_key)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("supabase load: {e}")))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(GwsError::Other(anyhow::anyhow!(
                "supabase load HTTP {status}: {text}"
            )));
        }
        let rows: Vec<MemoryTurn> = serde_json::from_str(&text)
            .map_err(|e| GwsError::Other(anyhow::anyhow!("supabase load parse: {e}")))?;
        Ok(rows)
    }

    async fn append(&self, conversation_id: &str, turn: &MemoryTurn) -> Result<(), GwsError> {
        let body = serde_json::json!([{
            "conversation_id": conversation_id,
            "role": turn.role,
            "content": turn.content,
        }]);
        let resp = self
            .client
            .post(self.endpoint())
            .header("apikey", &self.api_key)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .header("Prefer", "return=minimal")
            .json(&body)
            .send()
            .await
            .map_err(|e| GwsError::Other(anyhow::anyhow!("supabase append: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(GwsError::Other(anyhow::anyhow!(
                "supabase append HTTP {status}: {text}"
            )));
        }
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "supabase"
    }
}

/// Minimal URL encoder for query-string values (alphanumerics, `-_.~` pass
/// through; everything else is percent-encoded). Good enough for arbitrary
/// conversation IDs without pulling `form_urlencoded`.
fn urlencoding(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_turn_round_trip() {
        let m = ChatMessage::user("hello");
        let t = MemoryTurn::from_message(&m).unwrap();
        assert_eq!(t.role, "user");
        let back = t.to_message().unwrap();
        assert_eq!(back.role, Role::User);
        assert_eq!(back.content.as_deref(), Some("hello"));
    }

    #[test]
    fn memory_turn_skips_system_and_tool() {
        let s = ChatMessage::system("sys");
        assert!(MemoryTurn::from_message(&s).is_none());
        let t = ChatMessage::tool_result("id", "name", "result");
        assert!(MemoryTurn::from_message(&t).is_none());
    }

    #[tokio::test]
    async fn buffer_memory_per_conversation() {
        let mem = BufferMemory::with_capacity(0);
        mem.append(
            "a",
            &MemoryTurn {
                role: "user".into(),
                content: "hi".into(),
            },
        )
        .await
        .unwrap();
        mem.append(
            "b",
            &MemoryTurn {
                role: "user".into(),
                content: "yo".into(),
            },
        )
        .await
        .unwrap();
        let a = mem.load("a").await.unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].content, "hi");
        let b = mem.load("b").await.unwrap();
        assert_eq!(b[0].content, "yo");
    }

    #[tokio::test]
    async fn buffer_memory_trims_to_capacity() {
        let mem = BufferMemory::with_capacity(2);
        for i in 0..5 {
            mem.append(
                "c",
                &MemoryTurn {
                    role: "user".into(),
                    content: format!("m{i}"),
                },
            )
            .await
            .unwrap();
        }
        let out = mem.load("c").await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].content, "m3");
        assert_eq!(out[1].content, "m4");
    }

    #[test]
    fn urlencoding_escapes_specials() {
        assert_eq!(urlencoding("abc-_."), "abc-_.");
        assert_eq!(urlencoding("a b"), "a%20b");
        assert_eq!(urlencoding("eq."), "eq.");
        assert_eq!(urlencoding("a/b?c"), "a%2Fb%3Fc");
    }
}

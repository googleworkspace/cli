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

//! Concrete tool implementations exposed to the agent.
//!
//! Each tool is self-contained and only registered when its required
//! credentials are present. This keeps the model's tool list small and
//! avoids offering capabilities that would predictably fail.

pub mod browser;
pub mod gws;
pub mod notion;
pub mod supabase;
pub mod zapier;

use std::sync::Arc;

use crate::agent::config::AgentConfig;
use crate::agent::tool::{Tool, ToolRegistry};

/// Build the default registry by probing the environment for credentials
/// and registering the tools that can actually run.
pub fn default_registry(config: &AgentConfig) -> ToolRegistry {
    let mut reg = ToolRegistry::new();

    // Google Workspace: always available; the `gws` binary is us.
    reg.insert(Arc::new(gws::GwsTool::new()) as Arc<dyn Tool>);

    if let Some(t) = notion::NotionTool::from_env() {
        reg.insert(Arc::new(t) as Arc<dyn Tool>);
    }
    if let Some(t) = zapier::ZapierWebhookTool::from_env() {
        reg.insert(Arc::new(t) as Arc<dyn Tool>);
    }
    if let Some(t) = zapier::ZapierNlaTool::from_env() {
        reg.insert(Arc::new(t) as Arc<dyn Tool>);
    }
    if let Some(t) = supabase::SupabaseTool::from_config(config) {
        reg.insert(Arc::new(t) as Arc<dyn Tool>);
    }
    // Browser: registers a stub when the `browser-agent` feature is
    // disabled so the model gets a clear error rather than silent absence.
    reg.insert(Arc::new(browser::BrowserTool::new()) as Arc<dyn Tool>);

    reg
}

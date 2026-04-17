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

//! `gws agent` — a terminal AI agent that can call tools.
//!
//! This module wires together:
//!
//! * Config resolution ([`config`]).
//! * LLM provider (OpenRouter / Ollama via OpenAI-compatible REST, [`llm`]).
//! * Tool registry ([`tool`], [`tools`]).
//! * Conversation memory ([`memory`]).
//! * The agent loop itself ([`run_turn`]).
//!
//! The public entry point is [`handle_agent_command`], invoked from
//! `main.rs` when the user runs `gws agent ...`.

pub mod config;
pub mod llm;
pub mod memory;
pub mod tool;
pub mod tools;

use std::io::Write;
use std::sync::Arc;

use serde_json::Value;

use crate::error::GwsError;

use config::{parse_flags, AgentConfig, AgentOutputFormat};
use llm::{ChatMessage, LlmProvider, OpenAiCompatibleClient, Role, ToolCall};
use memory::{BufferMemory, Memory, MemoryTurn, SupabaseMemory};
use tool::{ToolError, ToolRegistry};

/// Summary of what a single turn produced; returned from [`run_turn`] and
/// used to drive the REPL / JSON output.
#[derive(Debug)]
pub struct TurnResult {
    pub answer: String,
    pub steps: u32,
    pub tool_calls: Vec<ExecutedTool>,
}

#[derive(Debug, Clone)]
pub struct ExecutedTool {
    pub name: String,
    pub arguments: String,
    pub result: Result<String, String>,
}

/// Entry point invoked from `main.rs` when the user runs `gws agent ...`.
pub async fn handle_agent_command(args: &[String]) -> Result<(), GwsError> {
    let flags = parse_flags(args)?;
    if flags.help {
        print_help();
        return Ok(());
    }
    let config = AgentConfig::resolve(flags)?;
    let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiCompatibleClient::new(config.clone())?);

    let memory: Arc<dyn Memory> = match SupabaseMemory::from_config(&config)? {
        Some(s) => Arc::new(s),
        None => Arc::new(BufferMemory::with_capacity(200)),
    };

    let mut registry = tools::default_registry(&config);
    registry.retain(&config.enabled_tools);

    if let Some(prompt) = config.one_shot.clone() {
        let result = run_turn(
            &config,
            provider.as_ref(),
            memory.as_ref(),
            &registry,
            &prompt,
        )
        .await?;
        emit_result(&config, &result);
        return Ok(());
    }

    run_repl(&config, provider, memory, registry).await
}

/// Print `gws agent --help` text.
fn print_help() {
    println!(
        "gws agent — terminal AI agent with tool use

USAGE:
    gws agent [FLAGS] [PROMPT...]

When a prompt is supplied, the agent answers once and exits. With no
prompt, it drops into a REPL; type `exit` or Ctrl-D to leave.

FLAGS:
    -h, --help                    Show this help.
    -p, --prompt <TEXT>           One-shot prompt (alternative to positional).
        --provider <NAME>         openrouter (default) or ollama.
        --model <MODEL>           Model id. Default: anthropic/claude-opus-4.6
                                  (OpenRouter) or llama3.1 (Ollama).
        --base-url <URL>          Override the provider base URL.
        --max-steps <N>           Max LLM/tool iterations per turn (default 8).
        --max-tokens <N>          Cap on response tokens.
        --temperature <F>         Sampling temperature.
        --system <TEXT>           Override the system prompt.
        --approve-tools           Ask for confirmation before each tool call.
        --output <FMT>            text (default) or json.
        --conversation <ID>       Reuse a persistent conversation id.
        --tools <CSV>             Comma-separated allow-list of tool names.
        --tool <NAME>             Repeatable tool allow-list entry.

ENVIRONMENT:
    OPENROUTER_API_KEY            OpenRouter auth.
    OLLAMA_BASE_URL               Override local Ollama host.
    NOTION_API_KEY                Enables the `notion` tool.
    ZAPIER_WEBHOOK_URL            Enables the `zapier_webhook` tool.
    ZAPIER_WEBHOOK_<NAME>_URL     Adds named hooks.
    ZAPIER_NLA_API_KEY            Enables the `zapier_actions` tool.
    SUPABASE_URL                  Enables Supabase tool + persistent memory
    SUPABASE_SERVICE_ROLE_KEY     (or SUPABASE_ANON_KEY).
    GWS_AGENT_MEMORY_TABLE        Override memory table (default gws_agent_memory).

EXAMPLES:
    gws agent 'Summarize my 5 most recent Gmail threads.'
    gws agent --provider ollama --model llama3.1 --approve-tools
    gws agent --conversation triage-2026 --prompt 'What did we decide?'
"
    );
}

/// Pretty-print a [`TurnResult`] according to the configured output format.
fn emit_result(config: &AgentConfig, result: &TurnResult) {
    match config.format() {
        AgentOutputFormat::Text => {
            println!("{}", result.answer);
        }
        AgentOutputFormat::Json => {
            let tool_calls: Vec<Value> = result
                .tool_calls
                .iter()
                .map(|t| {
                    let (ok, val) = match &t.result {
                        Ok(s) => (true, Value::String(s.clone())),
                        Err(e) => (false, Value::String(e.clone())),
                    };
                    serde_json::json!({
                        "tool": t.name,
                        "arguments": t.arguments,
                        "ok": ok,
                        "result": val,
                    })
                })
                .collect();
            let out = serde_json::json!({
                "answer": result.answer,
                "steps": result.steps,
                "tool_calls": tool_calls,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
            );
        }
    }
}

async fn run_repl(
    config: &AgentConfig,
    provider: Arc<dyn LlmProvider>,
    memory: Arc<dyn Memory>,
    registry: ToolRegistry,
) -> Result<(), GwsError> {
    let tools_line = if registry.is_empty() {
        "(no tools enabled)".to_string()
    } else {
        registry.names().join(", ")
    };
    eprintln!(
        "gws agent — provider={} model={} memory={} tools={}",
        config.provider.as_str(),
        config.model,
        memory.kind(),
        tools_line
    );
    eprintln!("Type 'exit' or press Ctrl-D to quit.");

    let stdin = std::io::stdin();
    loop {
        eprint!("you> ");
        std::io::stderr().flush().ok();
        let mut line = String::new();
        let n = stdin
            .read_line(&mut line)
            .map_err(|e| GwsError::Other(anyhow::anyhow!("stdin read failed: {e}")))?;
        if n == 0 {
            eprintln!();
            break;
        }
        let prompt = line.trim();
        if prompt.is_empty() {
            continue;
        }
        if matches!(prompt, "exit" | "quit" | ":q") {
            break;
        }
        match run_turn(
            config,
            provider.as_ref(),
            memory.as_ref(),
            &registry,
            prompt,
        )
        .await
        {
            Ok(result) => emit_result(config, &result),
            Err(e) => eprintln!("error: {e}"),
        }
    }
    Ok(())
}

/// Execute a single user turn. Public for testing of the tool-dispatch
/// loop via a mock [`LlmProvider`].
pub async fn run_turn(
    config: &AgentConfig,
    provider: &dyn LlmProvider,
    memory: &dyn Memory,
    registry: &ToolRegistry,
    prompt: &str,
) -> Result<TurnResult, GwsError> {
    let specs = registry.as_openai_specs();
    let mut messages: Vec<ChatMessage> = Vec::new();
    messages.push(ChatMessage::system(&config.system_prompt));

    // Replay prior turns.
    for t in memory.load(&config.conversation_id).await? {
        if let Some(m) = t.to_message() {
            messages.push(m);
        }
    }
    let user_msg = ChatMessage::user(prompt);
    messages.push(user_msg.clone());

    let mut executed: Vec<ExecutedTool> = Vec::new();
    let mut steps = 0u32;
    let mut final_answer: Option<String> = None;

    for _ in 0..config.max_steps {
        steps += 1;
        let resp = provider.complete(&messages, &specs).await?;

        if resp.tool_calls.is_empty() {
            final_answer = resp.content.clone();
            if let Some(ref text) = resp.content {
                messages.push(ChatMessage::assistant_text(text));
            } else {
                messages.push(ChatMessage::assistant_text(""));
            }
            break;
        }

        // Record assistant's tool_calls message so the tool responses can
        // reference it by id.
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: resp.content.clone(),
            tool_calls: Some(resp.tool_calls.clone()),
            tool_call_id: None,
            name: None,
        });

        for call in &resp.tool_calls {
            let (result, arg_str) = dispatch_tool(config, registry, call).await;
            messages.push(ChatMessage::tool_result(
                call.id.clone(),
                call.function.name.clone(),
                match &result {
                    Ok(s) => s.clone(),
                    Err(e) => format!("ERROR: {e}"),
                },
            ));
            executed.push(ExecutedTool {
                name: call.function.name.clone(),
                arguments: arg_str,
                result: result.map_err(|e| e.to_string()),
            });
        }
    }

    let answer = final_answer.unwrap_or_else(|| {
        "(agent stopped without producing a final answer — raise --max-steps?)".to_string()
    });

    // Persist this turn (user prompt + final answer only).
    if let Some(turn) = MemoryTurn::from_message(&user_msg) {
        memory.append(&config.conversation_id, &turn).await.ok();
    }
    let assistant_turn = MemoryTurn {
        role: "assistant".to_string(),
        content: answer.clone(),
    };
    memory
        .append(&config.conversation_id, &assistant_turn)
        .await
        .ok();

    Ok(TurnResult {
        answer,
        steps,
        tool_calls: executed,
    })
}

async fn dispatch_tool(
    config: &AgentConfig,
    registry: &ToolRegistry,
    call: &ToolCall,
) -> (Result<String, ToolError>, String) {
    let name = &call.function.name;
    let arg_str = call.function.arguments.clone();
    let Some(tool) = registry.get(name) else {
        return (
            Err(ToolError::runtime(format!("unknown tool '{name}'"))),
            arg_str,
        );
    };

    let args_value: Value = serde_json::from_str(&arg_str).unwrap_or_else(|_| {
        // Some models emit empty strings for no-arg tools; fall back to {}.
        if arg_str.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            Value::String(arg_str.clone())
        }
    });

    if config.approve_tools && !approve_interactive(name, &args_value) {
        return (Err(ToolError::Denied), arg_str);
    }

    let result = tool.call(args_value).await;
    (result, arg_str)
}

/// Ask the user on stderr whether to run a tool. Returns `true` when the
/// user answers y/yes/<enter>.
fn approve_interactive(name: &str, args: &Value) -> bool {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    eprintln!("tool> {name}({compact})");
    eprint!("approve? [Y/n] ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    let t = line.trim().to_ascii_lowercase();
    matches!(t.as_str(), "" | "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Scripted provider that returns queued responses in order.
    struct ScriptedProvider {
        script: Mutex<Vec<llm::LlmResponse>>,
    }

    impl ScriptedProvider {
        fn new(script: Vec<llm::LlmResponse>) -> Self {
            Self {
                script: Mutex::new(script),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn complete(
            &self,
            _messages: &[ChatMessage],
            _tools: &[Value],
        ) -> Result<llm::LlmResponse, GwsError> {
            let mut g = self.script.lock().unwrap();
            if g.is_empty() {
                return Err(GwsError::Other(anyhow::anyhow!("script exhausted")));
            }
            Ok(g.remove(0))
        }
        fn provider(&self) -> config::ProviderKind {
            config::ProviderKind::OpenRouter
        }
        fn model(&self) -> &str {
            "test-model"
        }
    }

    fn test_config() -> AgentConfig {
        AgentConfig {
            provider: config::ProviderKind::OpenRouter,
            model: "test".to_string(),
            base_url: "http://localhost".to_string(),
            api_key: None,
            app_title: "t".into(),
            app_referer: "t".into(),
            max_steps: 4,
            max_tokens: None,
            temperature: None,
            system_prompt: "be helpful".into(),
            approve_tools: false,
            one_shot: None,
            output_format: AgentOutputFormat::Text,
            supabase_url: None,
            supabase_key: None,
            supabase_memory_table: "gws_agent_memory".into(),
            conversation_id: "t1".into(),
            enabled_tools: vec![],
        }
    }

    struct FixedEcho;
    #[async_trait]
    impl tool::Tool for FixedEcho {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo"
        }
        fn parameters_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        async fn call(&self, args: Value) -> Result<String, ToolError> {
            Ok(args
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string())
        }
    }

    #[tokio::test]
    async fn single_turn_no_tools() {
        let cfg = test_config();
        let mem = BufferMemory::with_capacity(100);
        let reg = ToolRegistry::new();
        let provider = ScriptedProvider::new(vec![llm::LlmResponse {
            content: Some("Hello there.".into()),
            tool_calls: vec![],
            finish_reason: Some("stop".into()),
        }]);
        let result = run_turn(&cfg, &provider, &mem, &reg, "hi").await.unwrap();
        assert_eq!(result.answer, "Hello there.");
        assert_eq!(result.steps, 1);
        assert!(result.tool_calls.is_empty());

        // Memory has the user turn and assistant turn.
        let turns = mem.load(&cfg.conversation_id).await.unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[1].role, "assistant");
        assert_eq!(turns[1].content, "Hello there.");
    }

    #[tokio::test]
    async fn tool_call_then_answer() {
        let cfg = test_config();
        let mem = BufferMemory::with_capacity(100);
        let mut reg = ToolRegistry::new();
        reg.insert(Arc::new(FixedEcho));

        let provider = ScriptedProvider::new(vec![
            llm::LlmResponse {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: llm::ToolCallFunction {
                        name: "echo".into(),
                        arguments: "{\"text\":\"world\"}".into(),
                    },
                }],
                finish_reason: Some("tool_calls".into()),
            },
            llm::LlmResponse {
                content: Some("Got: world".into()),
                tool_calls: vec![],
                finish_reason: Some("stop".into()),
            },
        ]);

        let result = run_turn(&cfg, &provider, &mem, &reg, "echo").await.unwrap();
        assert_eq!(result.answer, "Got: world");
        assert_eq!(result.steps, 2);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "echo");
        assert!(result.tool_calls[0].result.is_ok());
    }

    #[tokio::test]
    async fn unknown_tool_surfaces_error_but_continues() {
        let cfg = test_config();
        let mem = BufferMemory::with_capacity(100);
        let reg = ToolRegistry::new();

        let provider = ScriptedProvider::new(vec![
            llm::LlmResponse {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: llm::ToolCallFunction {
                        name: "nope".into(),
                        arguments: "{}".into(),
                    },
                }],
                finish_reason: Some("tool_calls".into()),
            },
            llm::LlmResponse {
                content: Some("I can't do that.".into()),
                tool_calls: vec![],
                finish_reason: Some("stop".into()),
            },
        ]);
        let result = run_turn(&cfg, &provider, &mem, &reg, "?").await.unwrap();
        assert_eq!(result.answer, "I can't do that.");
        assert_eq!(result.tool_calls.len(), 1);
        assert!(result.tool_calls[0].result.is_err());
    }

    #[tokio::test]
    async fn exceeds_max_steps_returns_fallback_answer() {
        let mut cfg = test_config();
        cfg.max_steps = 1;
        let mem = BufferMemory::with_capacity(100);
        let mut reg = ToolRegistry::new();
        reg.insert(Arc::new(FixedEcho));

        // Only one response, which is a tool call → loop exits without
        // producing an answer.
        let provider = ScriptedProvider::new(vec![llm::LlmResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: llm::ToolCallFunction {
                    name: "echo".into(),
                    arguments: "{\"text\":\"x\"}".into(),
                },
            }],
            finish_reason: Some("tool_calls".into()),
        }]);
        let result = run_turn(&cfg, &provider, &mem, &reg, "loop").await.unwrap();
        assert!(result.answer.contains("max-steps"));
        assert_eq!(result.steps, 1);
    }
}

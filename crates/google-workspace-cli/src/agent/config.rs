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

//! Agent runtime configuration.
//!
//! Configuration is resolved from (in order of precedence):
//! 1. Command-line flags.
//! 2. Environment variables (`GWS_AGENT_*`, `OPENROUTER_API_KEY`, ...).
//! 3. Sensible defaults.
//!
//! No file-based configuration is read — agent settings are either explicit
//! flags or pulled from the environment so that credentials never end up on
//! disk implicitly.

use crate::error::GwsError;

/// Which LLM provider the agent talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenRouter (OpenAI-compatible router for Claude, GPT, Llama, Gemini, ...).
    OpenRouter,
    /// Local Ollama daemon exposing its OpenAI-compatible endpoint.
    Ollama,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Result<Self, GwsError> {
        match s.to_ascii_lowercase().as_str() {
            "openrouter" | "or" => Ok(ProviderKind::OpenRouter),
            "ollama" | "local" | "llama" => Ok(ProviderKind::Ollama),
            other => Err(GwsError::Validation(format!(
                "Unknown agent provider '{other}'. Valid values: openrouter, ollama."
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenRouter => "openrouter",
            ProviderKind::Ollama => "ollama",
        }
    }
}

/// Fully resolved agent configuration.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Referrer / app title headers sent to OpenRouter for usage attribution.
    pub app_title: String,
    pub app_referer: String,
    /// Maximum agent iterations (LLM call + tool execution cycles) per user turn.
    pub max_steps: u32,
    /// Maximum tokens for the model response.
    pub max_tokens: Option<u32>,
    /// Temperature override.
    pub temperature: Option<f32>,
    /// System prompt prepended to every conversation.
    pub system_prompt: String,
    /// If true, the agent asks for confirmation before executing any tool.
    pub approve_tools: bool,
    /// Single-shot prompt. When present, the agent runs one turn and exits.
    pub one_shot: Option<String>,
    /// Output format: "text" (default) or "json".
    pub output_format: AgentOutputFormat,
    /// Supabase URL used for persistent conversation memory. Falls back to
    /// in-process memory when unset.
    pub supabase_url: Option<String>,
    pub supabase_key: Option<String>,
    pub supabase_memory_table: String,
    /// Conversation identifier for multi-turn memory.
    pub conversation_id: String,
    /// Enabled tool names. Empty means "all available".
    pub enabled_tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentOutputFormat {
    Text,
    Json,
}

impl AgentOutputFormat {
    fn parse(s: &str) -> Result<Self, GwsError> {
        match s.to_ascii_lowercase().as_str() {
            "text" | "plain" => Ok(AgentOutputFormat::Text),
            "json" => Ok(AgentOutputFormat::Json),
            other => Err(GwsError::Validation(format!(
                "Unknown agent --output format '{other}'. Valid values: text, json."
            ))),
        }
    }
}

/// Default system prompt. Keep this concise; tools describe themselves.
const DEFAULT_SYSTEM_PROMPT: &str =
    "You are GWS Agent, a precise, concise command-line assistant that \
augments the Google Workspace CLI (`gws`). You can call tools to read and \
modify Google Workspace resources, query Notion, trigger Zapier hooks, read \
and write Supabase rows, and operate a headless browser. Always prefer using \
a tool when it can ground the answer in real data. When a tool fails, \
explain what happened and suggest next steps. Reply in plain text, no \
markdown headings.";

fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    })
}

/// Parsed CLI flags for `gws agent`.
#[derive(Default, Debug)]
pub(crate) struct RawFlags {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub prompt: Option<String>,
    pub max_steps: Option<u32>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub system: Option<String>,
    pub approve: bool,
    pub output: Option<String>,
    pub conversation: Option<String>,
    pub tools: Vec<String>,
    pub help: bool,
}

/// Parse the argument vector passed after `gws agent`.
pub(crate) fn parse_flags(args: &[String]) -> Result<RawFlags, GwsError> {
    let mut flags = RawFlags::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => flags.help = true,
            "--provider" => flags.provider = Some(take_value(&mut iter, "--provider")?),
            "--model" => flags.model = Some(take_value(&mut iter, "--model")?),
            "--base-url" => flags.base_url = Some(take_value(&mut iter, "--base-url")?),
            "-p" | "--prompt" => flags.prompt = Some(take_value(&mut iter, "--prompt")?),
            "--max-steps" => {
                flags.max_steps = Some(take_value(&mut iter, "--max-steps")?.parse().map_err(
                    |e: std::num::ParseIntError| {
                        GwsError::Validation(format!("Invalid --max-steps: {e}"))
                    },
                )?)
            }
            "--max-tokens" => {
                flags.max_tokens = Some(take_value(&mut iter, "--max-tokens")?.parse().map_err(
                    |e: std::num::ParseIntError| {
                        GwsError::Validation(format!("Invalid --max-tokens: {e}"))
                    },
                )?)
            }
            "--temperature" => {
                flags.temperature = Some(take_value(&mut iter, "--temperature")?.parse().map_err(
                    |e: std::num::ParseFloatError| {
                        GwsError::Validation(format!("Invalid --temperature: {e}"))
                    },
                )?)
            }
            "--system" => flags.system = Some(take_value(&mut iter, "--system")?),
            "--approve-tools" => flags.approve = true,
            "--output" => flags.output = Some(take_value(&mut iter, "--output")?),
            "--conversation" => flags.conversation = Some(take_value(&mut iter, "--conversation")?),
            "--tool" => flags.tools.push(take_value(&mut iter, "--tool")?),
            "--tools" => {
                let raw = take_value(&mut iter, "--tools")?;
                flags.tools.extend(
                    raw.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                );
            }
            other if other.starts_with("--") => {
                return Err(GwsError::Validation(format!(
                    "Unknown flag '{other}' for `gws agent`. Run `gws agent --help`."
                )));
            }
            // Positional fallback: concatenate free text as the one-shot prompt.
            other => {
                if let Some(prev) = flags.prompt.take() {
                    flags.prompt = Some(format!("{prev} {other}"));
                } else {
                    flags.prompt = Some(other.to_string());
                }
            }
        }
    }
    Ok(flags)
}

fn take_value<'a, I: Iterator<Item = &'a String>>(
    iter: &mut I,
    flag: &str,
) -> Result<String, GwsError> {
    iter.next()
        .cloned()
        .ok_or_else(|| GwsError::Validation(format!("Flag '{flag}' requires a value.")))
}

impl AgentConfig {
    /// Resolve a full config from parsed flags + environment.
    pub(crate) fn resolve(flags: RawFlags) -> Result<Self, GwsError> {
        let provider_str = flags
            .provider
            .or_else(|| env_trimmed("GWS_AGENT_PROVIDER"))
            .unwrap_or_else(|| "openrouter".to_string());
        let provider = ProviderKind::parse(&provider_str)?;

        let default_model = match provider {
            ProviderKind::OpenRouter => "anthropic/claude-opus-4.6",
            ProviderKind::Ollama => "llama3.1",
        };
        let model = flags
            .model
            .or_else(|| env_trimmed("GWS_AGENT_MODEL"))
            .unwrap_or_else(|| default_model.to_string());

        let default_base = match provider {
            ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
            ProviderKind::Ollama => "http://localhost:11434/v1",
        };
        let base_url = flags
            .base_url
            .or_else(|| env_trimmed("GWS_AGENT_BASE_URL"))
            .or_else(|| match provider {
                ProviderKind::OpenRouter => env_trimmed("OPENROUTER_BASE_URL"),
                ProviderKind::Ollama => env_trimmed("OLLAMA_BASE_URL"),
            })
            .unwrap_or_else(|| default_base.to_string());

        let api_key = match provider {
            ProviderKind::OpenRouter => env_trimmed("OPENROUTER_API_KEY"),
            ProviderKind::Ollama => env_trimmed("OLLAMA_API_KEY"),
        };

        let output_format = match flags.output.as_deref() {
            Some(s) => AgentOutputFormat::parse(s)?,
            None => AgentOutputFormat::Text,
        };

        let conversation_id = flags
            .conversation
            .or_else(|| env_trimmed("GWS_AGENT_CONVERSATION"))
            .unwrap_or_else(|| format!("cli-{}", uuid::Uuid::new_v4()));

        let system_prompt = flags
            .system
            .or_else(|| env_trimmed("GWS_AGENT_SYSTEM_PROMPT"))
            .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string());

        Ok(Self {
            provider,
            model,
            base_url,
            api_key,
            app_title: env_trimmed("GWS_AGENT_APP_TITLE")
                .unwrap_or_else(|| "google-workspace-cli".to_string()),
            app_referer: env_trimmed("GWS_AGENT_APP_REFERER")
                .unwrap_or_else(|| "https://github.com/googleworkspace/cli".to_string()),
            max_steps: flags.max_steps.unwrap_or(8),
            max_tokens: flags.max_tokens,
            temperature: flags.temperature,
            system_prompt,
            approve_tools: flags.approve,
            one_shot: flags.prompt,
            output_format,
            supabase_url: env_trimmed("SUPABASE_URL"),
            supabase_key: env_trimmed("SUPABASE_SERVICE_ROLE_KEY")
                .or_else(|| env_trimmed("SUPABASE_ANON_KEY")),
            supabase_memory_table: env_trimmed("GWS_AGENT_MEMORY_TABLE")
                .unwrap_or_else(|| "gws_agent_memory".to_string()),
            conversation_id,
            enabled_tools: flags.tools,
        })
    }

    pub fn format(&self) -> AgentOutputFormat {
        self.output_format
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_provider() {
        assert_eq!(
            ProviderKind::parse("OpenRouter").unwrap(),
            ProviderKind::OpenRouter
        );
        assert_eq!(ProviderKind::parse("ollama").unwrap(), ProviderKind::Ollama);
        assert_eq!(ProviderKind::parse("local").unwrap(), ProviderKind::Ollama);
        assert!(ProviderKind::parse("gpt5").is_err());
    }

    #[test]
    fn parse_output_format() {
        assert_eq!(
            AgentOutputFormat::parse("text").unwrap(),
            AgentOutputFormat::Text
        );
        assert_eq!(
            AgentOutputFormat::parse("JSON").unwrap(),
            AgentOutputFormat::Json
        );
        assert!(AgentOutputFormat::parse("yaml").is_err());
    }

    #[test]
    fn parse_flags_positional_becomes_prompt() {
        let f = parse_flags(&strs(&["what", "is", "2+2"])).unwrap();
        assert_eq!(f.prompt.as_deref(), Some("what is 2+2"));
        assert!(!f.help);
    }

    #[test]
    fn parse_flags_tools_csv() {
        let f = parse_flags(&strs(&["--tools", "notion,zapier", "--tool", "gws"])).unwrap();
        assert_eq!(f.tools, vec!["notion", "zapier", "gws"]);
    }

    #[test]
    fn parse_flags_requires_value() {
        let err = parse_flags(&strs(&["--model"])).unwrap_err();
        assert!(err.to_string().contains("--model"));
    }

    #[test]
    fn parse_flags_rejects_unknown() {
        let err = parse_flags(&strs(&["--nope"])).unwrap_err();
        assert!(err.to_string().contains("--nope"));
    }

    #[test]
    #[serial_test::serial]
    fn resolve_defaults_for_openrouter() {
        std::env::remove_var("GWS_AGENT_PROVIDER");
        std::env::remove_var("GWS_AGENT_MODEL");
        std::env::remove_var("GWS_AGENT_BASE_URL");
        std::env::remove_var("OPENROUTER_API_KEY");
        let cfg = AgentConfig::resolve(RawFlags::default()).unwrap();
        assert_eq!(cfg.provider, ProviderKind::OpenRouter);
        assert_eq!(cfg.base_url, "https://openrouter.ai/api/v1");
        assert!(cfg.model.starts_with("anthropic/"));
        assert!(cfg.conversation_id.starts_with("cli-"));
    }

    #[test]
    #[serial_test::serial]
    fn resolve_ollama_flag_wins_over_env() {
        std::env::set_var("GWS_AGENT_PROVIDER", "openrouter");
        let flags = RawFlags {
            provider: Some("ollama".to_string()),
            model: Some("llama3.1:70b".to_string()),
            ..Default::default()
        };
        let cfg = AgentConfig::resolve(flags).unwrap();
        std::env::remove_var("GWS_AGENT_PROVIDER");
        assert_eq!(cfg.provider, ProviderKind::Ollama);
        assert_eq!(cfg.model, "llama3.1:70b");
        assert_eq!(cfg.base_url, "http://localhost:11434/v1");
    }
}

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

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::executor;
use clap::{Arg, ArgMatches, Command};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;

pub struct ChatHelper;

impl Helper for ChatHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+send")
                .about("[Helper] Send a message to a space")
                .arg(
                    Arg::new("space")
                        .long("space")
                        .help("Space name (e.g. spaces/AAAA...)")
                        .required(true)
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("text")
                        .long("text")
                        .help("Message text (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws chat +send --space spaces/AAAAxxxx --text 'Hello team!'

TIPS:
  Use 'gws chat spaces list' to find space names.
  For cards or threaded replies, use the raw API instead.",
                ),
        );
        cmd
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        // We use `Box::pin` to create a pinned future on the heap.
        // This is necessary because the `Helper` trait returns a generic `Future`,
        // and async blocks in Rust are anonymous types that need to be erased
        // (via `dyn Future`) to be returned from a trait method.
        Box::pin(async move {
            if let Some(matches) = matches.subcommand_matches("+send") {
                // Parse arguments into our config struct config
                let config = parse_send_args(matches)?;
                // The `?` operator here will propagate any errors from `build_send_request`
                // immediately, returning `Err(GwsError)` from the async block.
                let (params_str, body_str, scopes) = build_send_request(&config, doc)?;

                let scope_strs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scope_strs).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) if matches.get_flag("dry-run") => (None, executor::AuthMethod::None),
                    Err(e) => return Err(GwsError::Auth(format!("Chat auth failed: {e}"))),
                };

                // Method: spaces.messages.create
                let spaces_res = doc.resources.get("spaces").ok_or_else(|| {
                    GwsError::Discovery("Resource 'spaces' not found".to_string())
                })?;
                let messages_res = spaces_res.resources.get("messages").ok_or_else(|| {
                    GwsError::Discovery("Resource 'spaces.messages' not found".to_string())
                })?;
                let create_method = messages_res.methods.get("create").ok_or_else(|| {
                    GwsError::Discovery("Method 'spaces.messages.create' not found".to_string())
                })?;

                let pagination = executor::PaginationConfig {
                    page_all: false,
                    page_limit: 10,
                    page_delay_ms: 100,
                };

                executor::execute_method(
                    doc,
                    create_method,
                    Some(&params_str),
                    Some(&body_str),
                    token.as_deref(),
                    auth_method,
                    None,
                    None,
                    None,
                    matches.get_flag("dry-run"),
                    &pagination,
                    None,
                    &crate::helpers::modelarmor::SanitizeMode::Warn,
                    &crate::formatter::OutputFormat::default(),
                    false,
                )
                .await?;

                return Ok(true);
            }
            Ok(false)
        })
    }
}

fn build_send_request(
    config: &SendConfig,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let spaces_res = doc
        .resources
        .get("spaces")
        .ok_or_else(|| GwsError::Discovery("Resource 'spaces' not found".to_string()))?;
    let messages_res = spaces_res
        .resources
        .get("messages")
        .ok_or_else(|| GwsError::Discovery("Resource 'spaces.messages' not found".to_string()))?;
    let create_method = messages_res.methods.get("create").ok_or_else(|| {
        GwsError::Discovery("Method 'spaces.messages.create' not found".to_string())
    })?;

    let params = json!({
        "parent": config.space
    });

    let body = json!({
        "text": config.text
    });

    let scopes: Vec<String> = create_method.scopes.iter().map(|s| s.to_string()).collect();

    Ok((params.to_string(), body.to_string(), scopes))
}

/// Configuration for sending a chat message.
///
/// This struct holds the parsed arguments for the `+send` command.
/// We use `String` here to own the data, as it will be used to construct
/// the JSON body for the API request.
pub struct SendConfig {
    /// The space to send the message to (e.g., "spaces/AAAA...").
    pub space: String,
    /// The text content of the message.
    pub text: String,
}

/// Parses the command line arguments into a `SendConfig` struct.
///
/// # Arguments
///
/// * `matches` - The `ArgMatches` from `clap` containing the parsed arguments.
///
/// # Returns
///
/// * `SendConfig` - The populated configuration struct.
pub fn parse_send_args(matches: &ArgMatches) -> Result<SendConfig, GwsError> {
    let space = matches.get_one::<String>("space").unwrap().clone();
    crate::validate::validate_resource_name(&space)?;

    Ok(SendConfig {
        space,
        text: matches.get_one::<String>("text").unwrap().clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{RestDescription, RestMethod, RestResource};
    use std::collections::HashMap;

    fn make_mock_doc() -> RestDescription {
        let mut methods = HashMap::new();
        methods.insert(
            "create".to_string(),
            RestMethod {
                scopes: vec!["https://scope".to_string()],
                ..Default::default()
            },
        );

        let mut messages_res = RestResource::default();
        messages_res.methods = methods;

        let mut spaces_res = RestResource::default();
        spaces_res
            .resources
            .insert("messages".to_string(), messages_res);

        let mut resources = HashMap::new();
        resources.insert("spaces".to_string(), spaces_res);

        RestDescription {
            resources,
            ..Default::default()
        }
    }

    fn make_matches_send(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("space").long("space"))
            .arg(Arg::new("text").long("text"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_build_send_request() {
        let doc = make_mock_doc();
        let config = SendConfig {
            space: "spaces/123".to_string(),
            text: "hello chat".to_string(),
        };
        let (params, body, scopes) = build_send_request(&config, &doc).unwrap();

        assert!(params.contains("spaces/123"));
        assert!(body.contains("hello chat"));
        assert_eq!(scopes[0], "https://scope");
    }

    #[test]
    fn test_parse_send_args() {
        let matches = make_matches_send(&["test", "--space", "valid-space", "--text", "t"]);
        let config = parse_send_args(&matches).unwrap();
        assert_eq!(config.space, "valid-space");
        assert_eq!(config.text, "t");
    }

    #[test]
    fn test_parse_send_args_rejects_traversal_in_space() {
        let matches = make_matches_send(&["test", "--space", "../etc/passwd", "--text", "t"]);
        let result = parse_send_args(&matches);
        assert!(
            result.is_err(),
            "space with path traversal should be rejected"
        );
    }

    #[test]
    fn test_parse_send_args_rejects_query_injection_in_space() {
        let matches =
            make_matches_send(&["test", "--space", "spaces/AAA?key=injected", "--text", "t"]);
        let result = parse_send_args(&matches);
        assert!(
            result.is_err(),
            "space with query characters should be rejected"
        );
    }

    #[test]
    fn test_inject_commands() {
        let helper = ChatHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();

        let cmd = helper.inject_commands(cmd, &doc);
        let subcommands: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subcommands.contains(&"+send"));
    }
}

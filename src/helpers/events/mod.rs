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
pub mod renew;
pub mod subscribe;

use renew::handle_renew;
use subscribe::handle_subscribe;

pub(super) use crate::auth;
pub(super) use crate::error::GwsError;
pub(super) use anyhow::Context;
pub(super) use clap::{Arg, ArgAction, ArgMatches, Command};
pub(super) use derive_builder::Builder;
pub(super) use serde_json::{json, Value};
pub(super) use std::future::Future;
pub(super) use std::pin::Pin;

pub struct EventsHelper;
pub(super) const PUBSUB_SCOPE: &str = "https://www.googleapis.com/auth/pubsub";
pub(super) const WORKSPACE_EVENTS_SCOPE: &str = "https://www.googleapis.com/auth/chat.spaces.readonly";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectId(pub String);
impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionName(pub String);
impl std::fmt::Display for SubscriptionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

#[derive(Debug, Clone, Builder)]
#[builder(setter(into))]
pub struct SubscribeConfig {
    #[builder(default)]
    pub target: Option<String>,
    #[builder(default)]
    pub event_types: Vec<String>,
    #[builder(default)]
    pub project: Option<ProjectId>,
    #[builder(default)]
    pub subscription: Option<SubscriptionName>,
    #[builder(default = "10")]
    pub max_messages: u32,
    #[builder(default = "5")]
    pub poll_interval: u64,
    #[builder(default = "false")]
    pub once: bool,
    #[builder(default = "false")]
    pub cleanup: bool,
    #[builder(default = "false")]
    pub no_ack: bool,
    #[builder(default)]
    pub output_dir: Option<String>,
}

impl Helper for EventsHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+subscribe")
                .about("[Helper] Subscribe to Workspace events and stream them as NDJSON")
                .arg(
                    Arg::new("target")
                        .long("target")
                        .help(
                            "Workspace resource URI (e.g., //chat.googleapis.com/spaces/SPACE_ID)",
                        )
                        .value_name("URI"),
                )
                .arg(
                    Arg::new("event-types")
                        .long("event-types")
                        .help("Comma-separated CloudEvents types to subscribe to")
                        .value_name("TYPES"),
                )
                .arg(
                    Arg::new("project")
                        .long("project")
                        .help("GCP project ID for Pub/Sub resources")
                        .value_name("PROJECT"),
                )
                .arg(
                    Arg::new("subscription")
                        .long("subscription")
                        .help("Existing Pub/Sub subscription name (skip setup)")
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("max-messages")
                        .long("max-messages")
                        .help("Max messages per pull batch (default: 10)")
                        .value_name("N")
                        .default_value("10"),
                )
                .arg(
                    Arg::new("poll-interval")
                        .long("poll-interval")
                        .help("Seconds between pulls (default: 5)")
                        .value_name("SECS")
                        .default_value("5"),
                )
                .arg(
                    Arg::new("once")
                        .long("once")
                        .help("Pull once and exit")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("cleanup")
                        .long("cleanup")
                        .help("Delete created Pub/Sub resources on exit")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("no-ack")
                        .long("no-ack")
                        .help("Don't auto-acknowledge messages")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("output-dir")
                        .long("output-dir")
                        .help("Write each event to a separate JSON file in this directory")
                        .value_name("DIR"),
                )
                .after_help("\
EXAMPLES:
  gws events +subscribe --target '//chat.googleapis.com/spaces/SPACE' --event-types 'google.workspace.chat.message.v1.created' --project my-project
  gws events +subscribe --subscription projects/p/subscriptions/my-sub --once
  gws events +subscribe ... --cleanup --output-dir ./events

TIPS:
  Without --cleanup, Pub/Sub resources persist for reconnection.
  Press Ctrl-C to stop gracefully."),
        );

        cmd = cmd.subcommand(
            Command::new("+renew")
                .about("[Helper] Renew/reactivate Workspace Events subscriptions")
                .arg(
                    Arg::new("name")
                        .long("name")
                        .help("Subscription name to reactivate (e.g., subscriptions/SUB_ID)")
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("all")
                        .long("all")
                        .help("Renew all subscriptions expiring within --within window")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("within")
                        .long("within")
                        .help("Time window for --all (e.g., 1h, 30m, 2d)")
                        .value_name("DURATION")
                        .default_value("1h"),
                )
                .after_help("\
EXAMPLES:
  gws events +renew --name subscriptions/SUB_ID
  gws events +renew --all --within 2d

TIPS:
  Subscriptions expire if not renewed periodically.
  Use --all with a cron job to keep subscriptions alive."),
        );

        cmd
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(sub_matches) = matches.subcommand_matches("+subscribe") {
                handle_subscribe(doc, sub_matches).await?;
                return Ok(true);
            }

            if let Some(renew_matches) = matches.subcommand_matches("+renew") {
                handle_renew(doc, renew_matches).await?;
                return Ok(true);
            }

            Ok(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_commands() {
        let helper = EventsHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();

        let cmd = helper.inject_commands(cmd, &doc);
        let subcommands: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subcommands.contains(&"+subscribe"));
        assert!(subcommands.contains(&"+renew"));
    }
}

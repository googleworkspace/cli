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

use crate::error::GwsError;
use clap::{ArgMatches, Command};
use std::future::Future;
use std::pin::Pin;
pub mod calendar;
pub mod chat;
pub mod docs;
pub mod drive;
pub mod events;
pub mod gmail;
pub mod modelarmor;
pub mod script;
pub mod sheets;
pub mod workflows;

/// Base URL for the Google Cloud Pub/Sub v1 API.
///
/// Shared across `events::subscribe` and `gmail::watch` so the constant
/// is defined in a single place.
pub(crate) const PUBSUB_API_BASE: &str = "https://pubsub.googleapis.com/v1";

/// Returns a future that completes when a shutdown signal is received.
///
/// On Unix this listens for both SIGINT (Ctrl+C) and SIGTERM; on other
/// platforms only SIGINT is handled. Used by long-running pull loops
/// (`gmail::watch`, `events::subscribe`) to exit cleanly under container
/// orchestrators (Kubernetes, Docker, systemd) that send SIGTERM.
///
/// The signal handler is registered once in a background task on first call
/// so it remains active for the lifetime of the process — no gap between
/// loop iterations.
pub(crate) async fn shutdown_signal() {
    use std::sync::OnceLock;
    use tokio::sync::Notify;

    static NOTIFY: OnceLock<std::sync::Arc<Notify>> = OnceLock::new();

    let notify = NOTIFY.get_or_init(|| {
        let n = std::sync::Arc::new(Notify::new());
        let n2 = n.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                match signal(SignalKind::terminate()) {
                    Ok(mut sigterm) => {
                        tokio::select! {
                            res = tokio::signal::ctrl_c() => {
                                res.expect("failed to listen for SIGINT");
                            }
                            Some(_) = sigterm.recv() => {}
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "warning: could not register SIGTERM handler: {e}. \
                             Listening for Ctrl+C only."
                        );
                        tokio::signal::ctrl_c()
                            .await
                            .expect("failed to listen for SIGINT");
                    }
                }
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c()
                    .await
                    .expect("failed to listen for SIGINT");
            }
            n2.notify_waiters();
        });
        n
    });

    notify.notified().await;
}

/// A trait for service-specific CLI helpers that inject custom commands.
pub trait Helper: Send + Sync {
    /// Injects subcommands into the service command.
    fn inject_commands(&self, cmd: Command, doc: &crate::discovery::RestDescription) -> Command;

    /// Attempts to handle a command. Returns Ok(Some(())) if handled,
    /// Ok(None) if not handled (should fall back to dynamic dispatch),
    /// or Err if handled but failed.
    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        sanitize_config: &'a modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>>;

    /// If true, only helper commands are shown (discovery-generated commands are suppressed).
    fn helper_only(&self) -> bool {
        false
    }
}

pub fn get_helper(service: &str) -> Option<Box<dyn Helper>> {
    match service {
        "gmail" => Some(Box::new(gmail::GmailHelper)),
        "sheets" => Some(Box::new(sheets::SheetsHelper)),
        "docs" => Some(Box::new(docs::DocsHelper)),
        "chat" => Some(Box::new(chat::ChatHelper)),
        "drive" => Some(Box::new(drive::DriveHelper)),
        "calendar" => Some(Box::new(calendar::CalendarHelper)),
        "script" | "apps-script" => Some(Box::new(script::ScriptHelper)),
        "workspaceevents" => Some(Box::new(events::EventsHelper)),
        "modelarmor" => Some(Box::new(modelarmor::ModelArmorHelper)),
        "workflow" => Some(Box::new(workflows::WorkflowHelper)),
        _ => None,
    }
}

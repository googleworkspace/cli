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

//! Structured Logging
//!
//! Provides opt-in, PII-free logging for HTTP requests and CLI operations.
//! All output goes to stderr or a log file — stdout remains clean for
//! machine-consumable JSON output.
//!
//! ## Environment Variables
//!
//! - `GOOGLE_WORKSPACE_CLI_LOG`: Filter directive for stderr logging
//!   (e.g., `gws=debug`, `gws=trace`). If unset, no stderr logging.
//!
//! - `GOOGLE_WORKSPACE_CLI_LOG_FILE`: Directory path for JSON-line log
//!   files with daily rotation. If unset, no file logging.

use tracing_subscriber::prelude::*;

/// Environment variable controlling stderr log output.
const ENV_LOG: &str = "GOOGLE_WORKSPACE_CLI_LOG";

/// Environment variable controlling file log output.
const ENV_LOG_FILE: &str = "GOOGLE_WORKSPACE_CLI_LOG_FILE";

/// Initialize the tracing subscriber based on environment variables.
///
/// If neither `GOOGLE_WORKSPACE_CLI_LOG` nor `GOOGLE_WORKSPACE_CLI_LOG_FILE`
/// is set, this is a no-op and logging adds zero overhead.
///
/// This function must be called at most once (typically in `main()`).
/// Subsequent calls will silently fail (tracing only allows one global
/// subscriber).
pub fn init_logging() {
    let stderr_filter = std::env::var(ENV_LOG).ok();
    let log_file_dir = std::env::var(ENV_LOG_FILE).ok();

    // If neither env var is set, skip initialization entirely for zero overhead.
    if stderr_filter.is_none() && log_file_dir.is_none() {
        return;
    }

    let registry = tracing_subscriber::registry();

    // Stderr layer: human-readable, filtered by GOOGLE_WORKSPACE_CLI_LOG
    let stderr_layer = stderr_filter.map(|filter| {
        let env_filter = tracing_subscriber::EnvFilter::new(filter);
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(false)
            .compact()
            .with_filter(env_filter)
    });

    // File layer: JSON-line output with daily rotation
    let (file_layer, _guard) = if let Some(ref dir) = log_file_dir {
        let file_appender = tracing_appender::rolling::daily(dir, "gws.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        let layer = tracing_subscriber::fmt::layer()
            .json()
            .with_writer(non_blocking)
            .with_target(true)
            .with_filter(tracing_subscriber::EnvFilter::new("gws=debug"));
        (Some(layer), Some(guard))
    } else {
        (None, None)
    };

    // Compose layers and set as global subscriber.
    // The guard is leaked intentionally so the non-blocking writer stays
    // alive for the lifetime of the process.
    let subscriber = registry.with(stderr_layer).with(file_layer);
    if tracing::subscriber::set_global_default(subscriber).is_ok() {
        if let Some(guard) = _guard {
            // Leak the guard so the non-blocking writer lives for the process lifetime.
            // This is the recommended pattern from tracing-appender docs.
            std::mem::forget(guard);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_logging_default_no_panic() {
        // With no env vars set, init_logging should be a no-op and not panic.
        // We can't truly test the global subscriber in unit tests (it's global state),
        // but we can verify the early-return path doesn't panic.
        std::env::remove_var(ENV_LOG);
        std::env::remove_var(ENV_LOG_FILE);
        init_logging();
    }

    #[test]
    fn test_env_var_names() {
        assert_eq!(ENV_LOG, "GOOGLE_WORKSPACE_CLI_LOG");
        assert_eq!(ENV_LOG_FILE, "GOOGLE_WORKSPACE_CLI_LOG_FILE");
    }
}

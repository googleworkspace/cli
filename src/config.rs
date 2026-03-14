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

//! Configuration directory resolution.
//!
//! Provides [`config_dir()`] to locate the gws config directory, respecting
//! the `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` environment variable and falling back
//! to `~/.config/gws` (or the OS-specific legacy path for existing installs).

use std::path::PathBuf;

/// Returns the gws configuration directory.
///
/// Resolution order:
/// 1. `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` environment variable (if set)
/// 2. `~/.config/gws` (if it exists — primary location)
/// 3. OS-specific config dir / `gws` (legacy fallback for existing installs)
/// 4. `~/.config/gws` (default for new installs)
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("GOOGLE_WORKSPACE_CLI_CONFIG_DIR") {
        return PathBuf::from(dir);
    }

    // Use ~/.config/gws on all platforms for a consistent, user-friendly path.
    let primary = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("gws");
    if primary.exists() {
        return primary;
    }

    // Backward compat: fall back to OS-specific config dir for existing installs
    // (e.g. ~/Library/Application Support/gws on macOS, %APPDATA%\gws on Windows).
    let legacy = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gws");
    if legacy.exists() {
        return legacy;
    }

    primary
}

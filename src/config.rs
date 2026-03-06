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

use std::path::PathBuf;

/// Returns the gws configuration directory.
///
/// Prefers `~/.config/gws` for a consistent, cross-platform path.
/// Falls back to the OS-specific config directory (e.g. `~/Library/Application Support/gws`
/// on macOS) for backward compatibility with existing installs.
///
/// The `GOOGLE_WORKSPACE_CLI_CONFIG_DIR` environment variable overrides the default.
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

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

//! Discovery Document types and fetching.
//!
//! Types are re-exported from the `google_workspace` library crate.
//! The CLI wrapper provides default caching via `config_dir()`.

pub use google_workspace::discovery::*;

/// Fetches and caches a Google Discovery Document using the CLI's config directory.
///
/// This is a convenience wrapper around
/// [`google_workspace::discovery::fetch_discovery_document`] that automatically
/// uses the CLI's cache directory (`~/.config/gws/cache/`).
pub async fn fetch_discovery_document(
    service: &str,
    version: &str,
) -> anyhow::Result<RestDescription> {
    let cache_dir = crate::auth_commands::config_dir().join("cache");
    google_workspace::discovery::fetch_discovery_document(service, version, Some(&cache_dir)).await
}

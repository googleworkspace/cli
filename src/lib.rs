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

//! Google Workspace CLI — library crate.
//!
//! This crate exposes a focused subset of `gws` internals for programmatic
//! access to Google Workspace APIs. Library consumers can introspect API
//! schemas, resolve service names, validate inputs, and build authenticated
//! HTTP clients without depending on any CLI-specific code (clap, ratatui,
//! interactive OAuth flows, etc.).
//!
//! # Exposed modules
//!
//! | Module | Purpose |
//! |---|---|
//! | [`discovery`] | `RestDescription`, `RestMethod`, `RestResource` — introspect Google APIs |
//! | [`error`] | `GwsError` — unified error type |
//! | [`config`] | `config_dir()` — resolve the gws configuration directory |
//! | [`services`] | `resolve_service()`, `SERVICES` — service name to API mapping |
//! | [`validate`] | `validate_api_identifier()`, input safety helpers |
//! | [`client`] | `build_client()`, `send_with_retry()` — HTTP with retry |

pub mod client;
pub mod config;
pub mod discovery;
pub mod error;
pub mod services;
pub mod validate;

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
//! Provides programmatic access to the core functionality of `gws`:
//! Discovery Document parsing, OAuth / Service Account authentication,
//! API execution, and input validation.
//!
//! The binary (`gws`) re-declares these same modules; both targets compile
//! from the same source files (dual compilation).

// Internal modules are shared with the binary via dual compilation.
// They appear unused from the library's perspective but are needed by the bin target.
#![allow(dead_code)]

pub mod auth;
pub(crate) mod auth_commands;
pub mod client;
pub mod commands;
pub mod config;
pub mod credential_store;
pub mod discovery;
pub mod error;
pub mod executor;
pub mod formatter;
pub mod fs_util;
pub(crate) mod generate_skills;
pub(crate) mod helpers;
pub(crate) mod mcp_server;
pub mod oauth_config;
pub mod sanitize;
pub(crate) mod schema;
pub mod services;
pub(crate) mod setup;
pub(crate) mod setup_tui;
pub(crate) mod text;
pub mod token_storage;
pub mod validate;

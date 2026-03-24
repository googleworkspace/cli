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

//! Google Workspace API client library.
//!
//! Provides types and utilities for working with Google Workspace APIs
//! via the [Discovery Service](https://developers.google.com/discovery).
//!
//! # Modules
//!
//! - [`discovery`] — Discovery Document types and fetching
//! - [`error`] — Structured error types
//! - [`services`] — Service name registry and resolution
//! - [`validate`] — Input validation and URL encoding utilities
//! - [`client`] — HTTP client with retry logic

pub mod client;
pub mod discovery;
pub mod error;
pub mod services;
pub mod validate;

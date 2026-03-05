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

//! Integration tests verifying that the library crate exposes key types
//! and functions. These are offline tests — no network calls.

use gws::discovery::RestDescription;
use gws::error::GwsError;
use gws::services::resolve_service;
use gws::validate::validate_api_identifier;

#[test]
fn rest_description_deserializes_minimal() {
    let json = r#"{"name":"test","version":"v1","rootUrl":"https://example.com/"}"#;
    let doc: RestDescription = serde_json::from_str(json).unwrap();
    assert_eq!(doc.name, "test");
    assert_eq!(doc.version, "v1");
}

#[test]
fn resolve_service_returns_known() {
    let (api, ver) = resolve_service("drive").unwrap();
    assert_eq!(api, "drive");
    assert_eq!(ver, "v3");
}

#[test]
fn resolve_service_rejects_unknown() {
    assert!(resolve_service("nonexistent").is_err());
}

#[test]
fn gws_error_variants_exist() {
    let err = GwsError::Validation("test".to_string());
    let json = err.to_json();
    assert_eq!(json["error"]["code"], 400);
}

#[test]
fn validate_api_identifier_accepts_valid() {
    assert!(validate_api_identifier("drive").is_ok());
    assert!(validate_api_identifier("v3").is_ok());
}

#[test]
fn validate_api_identifier_rejects_traversal() {
    assert!(validate_api_identifier("../etc").is_err());
}

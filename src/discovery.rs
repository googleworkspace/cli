#![allow(dead_code)]
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

//! Discovery Document Parsing and Management
//!
//! Handles fetching, caching, and parsing Google API Discovery Documents.
//! These JSON schemas define the shapes of API requests and responses, forming
//! the foundation of the dynamically generated CLI commands.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level Discovery REST Description document.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestDescription {
    pub name: String,
    pub version: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub root_url: String,
    #[serde(default)]
    pub service_path: String,
    pub base_url: Option<String>,
    #[serde(default)]
    pub schemas: HashMap<String, JsonSchema>,
    #[serde(default)]
    pub resources: HashMap<String, RestResource>,
    #[serde(default)]
    pub parameters: HashMap<String, MethodParameter>,
    pub auth: Option<AuthDescription>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AuthDescription {
    pub oauth2: Option<OAuth2Description>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OAuth2Description {
    pub scopes: Option<HashMap<String, ScopeDescription>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ScopeDescription {
    pub description: Option<String>,
}

/// A resource in the Discovery Document, which can contain methods and nested sub-resources.
#[derive(Debug, Deserialize, Default)]
pub struct RestResource {
    #[serde(default)]
    pub methods: HashMap<String, RestMethod>,
    #[serde(default)]
    pub resources: HashMap<String, RestResource>,
}

/// A single API method.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestMethod {
    pub id: Option<String>,
    pub description: Option<String>,
    pub http_method: String,
    pub path: String,
    #[serde(default)]
    pub parameters: HashMap<String, MethodParameter>,
    #[serde(default)]
    pub parameter_order: Vec<String>,
    pub request: Option<SchemaRef>,
    pub response: Option<SchemaRef>,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub flat_path: Option<String>,
    #[serde(default)]
    pub supports_media_download: bool,
    #[serde(default)]
    pub supports_media_upload: bool,
    pub media_upload: Option<MediaUpload>,
}

/// Media upload metadata from the Discovery Document.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUpload {
    pub protocols: Option<MediaUploadProtocols>,
    pub accept: Option<Vec<String>>,
}

/// Upload protocol details.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUploadProtocols {
    pub simple: Option<MediaUploadProtocol>,
}

/// A single upload protocol entry.
#[derive(Debug, Deserialize, Default)]
pub struct MediaUploadProtocol {
    pub path: String,
    pub multipart: Option<bool>,
}

/// A reference to a schema (e.g., `{ "$ref": "File" }`).
#[derive(Debug, Deserialize, Default)]
pub struct SchemaRef {
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    #[serde(rename = "parameterName")]
    pub parameter_name: Option<String>,
}

/// A parameter definition for a method.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MethodParameter {
    #[serde(rename = "type")]
    pub param_type: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub format: Option<String>,
    pub default: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub enum_descriptions: Option<Vec<String>>,
    #[serde(default)]
    pub repeated: bool,
    pub minimum: Option<String>,
    pub maximum: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
}

/// JSON Schema definition for request/response bodies.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchema {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub schema_type: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub properties: HashMap<String, JsonSchemaProperty>,
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    pub items: Option<Box<JsonSchemaProperty>>,
    #[serde(default)]
    pub required: Vec<String>,
    pub additional_properties: Option<Box<JsonSchemaProperty>>,
}

/// A property within a JSON Schema.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchemaProperty {
    #[serde(rename = "type")]
    pub prop_type: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "$ref")]
    pub schema_ref: Option<String>,
    pub format: Option<String>,
    pub items: Option<Box<JsonSchemaProperty>>,
    #[serde(default)]
    pub properties: HashMap<String, JsonSchemaProperty>,
    #[serde(default)]
    pub read_only: bool,
    pub default: Option<String>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    pub additional_properties: Option<Box<JsonSchemaProperty>>,
}

fn apply_api_base_override(doc: &mut RestDescription, api_base_override: &Option<String>) {
    if let Some(ref override_url) = api_base_override {
        let base = override_url.trim_end_matches('/');
        doc.root_url = format!("{base}/");
        doc.base_url = Some(format!("{base}/{}", doc.service_path));
    }
}

/// Fetches and caches a Google Discovery Document.
pub async fn fetch_discovery_document(
    service: &str,
    version: &str,
) -> anyhow::Result<RestDescription> {
    // Validate service and version to prevent path traversal in cache filenames
    // and injection in discovery URLs.
    let service =
        crate::validate::validate_api_identifier(service).map_err(|e| anyhow::anyhow!("{e}"))?;
    let version =
        crate::validate::validate_api_identifier(version).map_err(|e| anyhow::anyhow!("{e}"))?;

    let cache_dir = crate::auth_commands::config_dir().join("cache");
    std::fs::create_dir_all(&cache_dir)?;

    let cache_file = cache_dir.join(format!("{service}_{version}.json"));

    let api_base_override = std::env::var("GOOGLE_WORKSPACE_CLI_API_BASE").ok();

    // Check cache (24hr TTL)
    if cache_file.exists() {
        if let Ok(metadata) = std::fs::metadata(&cache_file) {
            if let Ok(modified) = metadata.modified() {
                if modified.elapsed().unwrap_or_default() < std::time::Duration::from_secs(86400) {
                    let data = std::fs::read_to_string(&cache_file)?;
                    let mut doc: RestDescription = serde_json::from_str(&data)?;
                    apply_api_base_override(&mut doc, &api_base_override);
                    return Ok(doc);
                }
            }
        }
    }

    let discovery_base = api_base_override
        .as_deref()
        .unwrap_or("https://www.googleapis.com");

    let url = format!(
        "{}/discovery/v1/apis/{}/{}/rest",
        discovery_base,
        crate::validate::encode_path_segment(service),
        crate::validate::encode_path_segment(version),
    );

    let client = crate::client::build_client()?;
    let resp = client.get(&url).send().await?;

    let body = if resp.status().is_success() {
        resp.text().await?
    } else {
        // Try the $discovery/rest URL pattern used by newer APIs (Forms, Keep, Meet, etc.)
        // When an override is set, preserve the service name in the path so the proxy
        // can route to the correct upstream.
        let alt_url = if let Some(ref base) = api_base_override {
            format!("{}/$discovery/rest", base.trim_end_matches('/'))
        } else {
            format!("https://{service}.googleapis.com/$discovery/rest")
        };
        let mut alt_req = client.get(&alt_url).query(&[("version", version)]);
        if api_base_override.is_some() {
            alt_req = alt_req.query(&[("service", service)]);
        }
        let alt_resp = alt_req.send().await?;
        if !alt_resp.status().is_success() {
            anyhow::bail!(
                "Failed to fetch Discovery Document for {service}/{version}: HTTP {} (tried both standard and $discovery URLs)",
                alt_resp.status()
            );
        }
        alt_resp.text().await?
    };

    // Write to cache
    if let Err(e) = std::fs::write(&cache_file, &body) {
        let _ = e;
    }

    let mut doc: RestDescription = serde_json::from_str(&body)?;
    apply_api_base_override(&mut doc, &api_base_override);

    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_rest_description() {
        let json = r#"{
            "name": "drive",
            "version": "v3",
            "rootUrl": "https://www.googleapis.com/",
            "servicePath": "drive/v3/",
            "resources": {
                "files": {
                    "methods": {
                        "list": {
                            "httpMethod": "GET",
                            "path": "files",
                            "response": { "$ref": "FileList" }
                        }
                    }
                }
            },
            "schemas": {
                "FileList": {
                    "id": "FileList",
                    "type": "object",
                    "properties": {
                        "files": {
                            "type": "array",
                            "items": { "$ref": "File" }
                        }
                    }
                }
            }
        }"#;

        let doc: RestDescription = serde_json::from_str(json).unwrap();
        assert_eq!(doc.name, "drive");
        assert_eq!(doc.version, "v3");
        assert_eq!(doc.root_url, "https://www.googleapis.com/");
        assert_eq!(doc.service_path, "drive/v3/");

        // precise resource checking
        let files = doc.resources.get("files").expect("files resource missing");
        let list = files.methods.get("list").expect("list method missing");
        assert_eq!(list.http_method, "GET");
        assert_eq!(list.path, "files");

        // schema checking
        let file_list = doc
            .schemas
            .get("FileList")
            .expect("FileList schema missing");
        assert_eq!(file_list.id.as_deref(), Some("FileList"));
    }

    #[test]
    fn test_deserialize_defaults() {
        let json = r#"{
            "name": "admin",
            "version": "directory_v1",
            "rootUrl": "https://admin.googleapis.com/"
        }"#;

        let doc: RestDescription = serde_json::from_str(json).unwrap();
        assert_eq!(doc.service_path, ""); // default empty string
        assert!(doc.resources.is_empty());
        assert!(doc.schemas.is_empty());
    }

    struct EnvVarGuard {
        name: String,
        original: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(name: &str, value: &str) -> Self {
            let original = std::env::var_os(name);
            std::env::set_var(name, value);
            Self {
                name: name.to_string(),
                original,
            }
        }

        fn remove(name: &str) -> Self {
            let original = std::env::var_os(name);
            std::env::remove_var(name);
            Self {
                name: name.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => std::env::set_var(&self.name, v),
                None => std::env::remove_var(&self.name),
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_api_base_override_rewrites_doc() {
        let _guard = EnvVarGuard::set(
            "GOOGLE_WORKSPACE_CLI_API_BASE",
            "https://my-proxy.example.com",
        );

        let doc_json = r#"{
            "name": "test",
            "version": "v1",
            "rootUrl": "https://www.googleapis.com/",
            "servicePath": "test/v1/"
        }"#;
        let mut doc: RestDescription = serde_json::from_str(doc_json).unwrap();

        let api_base_override = std::env::var("GOOGLE_WORKSPACE_CLI_API_BASE").ok();
        apply_api_base_override(&mut doc, &api_base_override);

        assert_eq!(doc.root_url, "https://my-proxy.example.com/");
        assert_eq!(
            doc.base_url.as_deref(),
            Some("https://my-proxy.example.com/test/v1/")
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_api_base_override_unset_preserves_original() {
        let _guard = EnvVarGuard::remove("GOOGLE_WORKSPACE_CLI_API_BASE");

        let doc_json = r#"{
            "name": "drive",
            "version": "v3",
            "rootUrl": "https://www.googleapis.com/",
            "servicePath": "drive/v3/"
        }"#;

        let mut doc: RestDescription = serde_json::from_str(doc_json).unwrap();

        let api_base_override = std::env::var("GOOGLE_WORKSPACE_CLI_API_BASE").ok();
        apply_api_base_override(&mut doc, &api_base_override);

        assert_eq!(doc.root_url, "https://www.googleapis.com/");
        assert!(doc.base_url.is_none());
    }

    #[test]
    fn test_api_base_override_strips_trailing_slash() {
        let mut doc: RestDescription = serde_json::from_str(
            r#"{"name": "test", "version": "v1", "rootUrl": "https://x.com/", "servicePath": "t/v1/"}"#,
        )
        .unwrap();

        let override_url = Some("https://my-proxy.example.com/".to_string());
        apply_api_base_override(&mut doc, &override_url);

        assert_eq!(doc.root_url, "https://my-proxy.example.com/");
        assert_eq!(
            doc.base_url.as_deref(),
            Some("https://my-proxy.example.com/t/v1/")
        );
    }
}

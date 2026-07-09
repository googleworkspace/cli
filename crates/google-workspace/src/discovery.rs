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

use anyhow::Context;
use serde::Deserialize;

/// Environment variable for overriding Discovery-derived API request endpoints.
pub const API_ENDPOINT_BASE_URL_ENV: &str = "GOOGLE_WORKSPACE_CLI_API_ENDPOINT_BASE_URL";

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

/// Returns the configured API endpoint base URL, if one was provided.
pub fn api_endpoint_base_url_from_env() -> Option<String> {
    std::env::var(API_ENDPOINT_BASE_URL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Rewrites Discovery API endpoints to use a configured endpoint origin.
///
/// Only the scheme, host, and port are replaced. Existing paths from the
/// Discovery document are preserved so service-specific routing remains
/// Discovery-driven. Discovery fetch URLs themselves are not rewritten.
pub fn rewrite_api_urls_for_endpoint_base_url(
    mut doc: RestDescription,
    endpoint_base_url: Option<&str>,
) -> anyhow::Result<RestDescription> {
    let endpoint_base_url = match endpoint_base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return Ok(doc),
    };

    let endpoint_origin = parse_endpoint_base_url_origin(endpoint_base_url)?;
    doc.root_url = rewrite_url_origin(&doc.root_url, &endpoint_origin)?;
    if let Some(base_url) = doc.base_url.as_mut() {
        let rewritten_base_url = rewrite_url_origin(base_url, &endpoint_origin)?;
        *base_url = rewritten_base_url;
    }

    Ok(doc)
}

fn parse_endpoint_base_url_origin(endpoint_base_url: &str) -> anyhow::Result<reqwest::Url> {
    let url = reqwest::Url::parse(endpoint_base_url)
        .with_context(|| format!("Invalid {API_ENDPOINT_BASE_URL_ENV}: {endpoint_base_url}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        anyhow::bail!("{API_ENDPOINT_BASE_URL_ENV} must use http or https");
    }
    Ok(url)
}

fn rewrite_url_origin(url: &str, endpoint_origin: &reqwest::Url) -> anyhow::Result<String> {
    let mut rewritten = reqwest::Url::parse(url)
        .with_context(|| format!("Discovery document contains an invalid API URL: {url}"))?;
    rewritten
        .set_scheme(endpoint_origin.scheme())
        .map_err(|_| {
            anyhow::anyhow!(
                "Invalid endpoint base URL scheme: {}",
                endpoint_origin.scheme()
            )
        })?;
    rewritten
        .set_host(endpoint_origin.host_str())
        .context("Failed to apply endpoint host to Discovery API URL")?;
    rewritten
        .set_port(endpoint_origin.port())
        .map_err(|_| anyhow::anyhow!("Failed to apply endpoint port to Discovery API URL"))?;
    Ok(rewritten.to_string())
}

/// Fetches and caches a Google Discovery Document.
///
/// When `cache_dir` is `Some`, the document is cached on disk with a 24-hour
/// TTL. Pass `None` to skip caching entirely.
pub async fn fetch_discovery_document(
    service: &str,
    version: &str,
    cache_dir: Option<&std::path::Path>,
) -> anyhow::Result<RestDescription> {
    // Validate service and version to prevent path traversal in cache filenames
    // and injection in discovery URLs.
    let service =
        crate::validate::validate_api_identifier(service).map_err(|e| anyhow::anyhow!("{e}"))?;
    let version =
        crate::validate::validate_api_identifier(version).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Check cache (24hr TTL)
    if let Some(dir) = cache_dir {
        tokio::fs::create_dir_all(dir).await?;
        let cache_file = dir.join(format!("{service}_{version}.json"));

        if let Ok(metadata) = tokio::fs::metadata(&cache_file).await {
            if let Ok(modified) = metadata.modified() {
                if modified.elapsed().unwrap_or_default() < std::time::Duration::from_secs(86400) {
                    let data = tokio::fs::read_to_string(&cache_file).await?;
                    let doc: RestDescription = serde_json::from_str(&data)?;
                    tracing::debug!(service = %service, version = %version, "Discovery cache hit");
                    return rewrite_api_urls_for_endpoint_base_url(
                        doc,
                        api_endpoint_base_url_from_env().as_deref(),
                    );
                }
            }
        }
    }

    let url = format!(
        "https://www.googleapis.com/discovery/v1/apis/{}/{}/rest",
        crate::validate::encode_path_segment(service),
        crate::validate::encode_path_segment(version),
    );

    tracing::debug!(service = %service, version = %version, "Fetching discovery document");
    let client = crate::client::build_client()?;
    let resp = client.get(&url).send().await?;

    let body = if resp.status().is_success() {
        resp.text().await?
    } else {
        // Try the $discovery/rest URL pattern used by newer APIs (Forms, Keep, Meet, etc.)
        let alt_url = format!("https://{service}.googleapis.com/$discovery/rest");
        let alt_resp = client
            .get(&alt_url)
            .query(&[("version", version)])
            .send()
            .await?;
        if !alt_resp.status().is_success() {
            anyhow::bail!(
                "Failed to fetch Discovery Document for {service}/{version}: HTTP {} (tried both standard and $discovery URLs)",
                alt_resp.status()
            );
        }
        alt_resp.text().await?
    };

    // Write to cache
    if let Some(dir) = cache_dir {
        let cache_file = dir.join(format!("{service}_{version}.json"));
        if let Err(e) = tokio::fs::write(&cache_file, &body).await {
            tracing::warn!(error = %e, "Failed to write discovery cache");
        }
    }

    let doc: RestDescription = serde_json::from_str(&body)?;
    rewrite_api_urls_for_endpoint_base_url(doc, api_endpoint_base_url_from_env().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn remove(key: &'static str) -> Self {
            let original = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, original }
        }

        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn minimal_doc(root_url: &str, service_path: &str, base_url: Option<&str>) -> RestDescription {
        RestDescription {
            name: "drive".to_string(),
            version: "v3".to_string(),
            root_url: root_url.to_string(),
            service_path: service_path.to_string(),
            base_url: base_url.map(str::to_string),
            ..RestDescription::default()
        }
    }

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

    #[test]
    fn test_rewrite_api_urls_for_endpoint_base_url_none_noop() {
        let doc = minimal_doc(
            "https://www.googleapis.com/",
            "drive/v3/",
            Some("https://www.googleapis.com/drive/v3/"),
        );

        let doc = rewrite_api_urls_for_endpoint_base_url(doc, None).unwrap();

        assert_eq!(doc.root_url, "https://www.googleapis.com/");
        assert_eq!(doc.service_path, "drive/v3/");
        assert_eq!(
            doc.base_url.as_deref(),
            Some("https://www.googleapis.com/drive/v3/")
        );
    }

    #[test]
    fn test_rewrite_api_urls_for_endpoint_base_url_empty_string_noop() {
        let doc = minimal_doc(
            "https://www.googleapis.com/",
            "drive/v3/",
            Some("https://www.googleapis.com/drive/v3/"),
        );

        let doc = rewrite_api_urls_for_endpoint_base_url(doc, Some("  ")).unwrap();

        assert_eq!(doc.root_url, "https://www.googleapis.com/");
        assert_eq!(
            doc.base_url.as_deref(),
            Some("https://www.googleapis.com/drive/v3/")
        );
    }

    #[test]
    fn test_rewrite_api_urls_for_endpoint_base_url_replaces_root_url_without_base_url() {
        let doc = minimal_doc("https://www.googleapis.com/", "drive/v3/", None);

        let doc = rewrite_api_urls_for_endpoint_base_url(
            doc,
            Some("https://proxy.example.com/api-gateway/"),
        )
        .unwrap();

        assert_eq!(doc.root_url, "https://proxy.example.com/");
        assert_eq!(doc.service_path, "drive/v3/");
        assert!(doc.base_url.is_none());
    }

    #[test]
    fn test_rewrite_api_urls_for_endpoint_base_url_replaces_base_url_and_preserves_paths() {
        let doc = minimal_doc(
            "https://www.googleapis.com/",
            "drive/v3/",
            Some("https://sheets.googleapis.com/v4/"),
        );

        let doc =
            rewrite_api_urls_for_endpoint_base_url(doc, Some("http://proxy.example.com:8080"))
                .unwrap();

        assert_eq!(doc.root_url, "http://proxy.example.com:8080/");
        assert_eq!(
            doc.base_url.as_deref(),
            Some("http://proxy.example.com:8080/v4/")
        );
        assert_eq!(doc.service_path, "drive/v3/");
    }

    #[test]
    fn test_rewrite_api_urls_for_endpoint_base_url_trailing_slash_is_normalized() {
        let without_slash = rewrite_api_urls_for_endpoint_base_url(
            minimal_doc(
                "https://www.googleapis.com/",
                "drive/v3/",
                Some("https://www.googleapis.com/drive/v3/"),
            ),
            Some("https://proxy.example.com"),
        )
        .unwrap();
        let with_slash = rewrite_api_urls_for_endpoint_base_url(
            minimal_doc(
                "https://www.googleapis.com/",
                "drive/v3/",
                Some("https://www.googleapis.com/drive/v3/"),
            ),
            Some("https://proxy.example.com/"),
        )
        .unwrap();

        assert_eq!(without_slash.root_url, with_slash.root_url);
        assert_eq!(without_slash.base_url, with_slash.base_url);
        assert_eq!(without_slash.root_url, "https://proxy.example.com/");
        assert_eq!(
            without_slash.base_url.as_deref(),
            Some("https://proxy.example.com/drive/v3/")
        );
    }

    #[test]
    fn test_rewrite_api_urls_for_endpoint_base_url_preserves_original_url_paths() {
        let doc = minimal_doc(
            "https://analyticsadmin.googleapis.com/v1beta/",
            "",
            Some("https://analyticsdata.googleapis.com/v1beta/"),
        );

        let doc = rewrite_api_urls_for_endpoint_base_url(
            doc,
            Some("https://proxy.example.com/proxy-prefix/"),
        )
        .unwrap();

        assert_eq!(doc.root_url, "https://proxy.example.com/v1beta/");
        assert_eq!(
            doc.base_url.as_deref(),
            Some("https://proxy.example.com/v1beta/")
        );
    }

    #[test]
    fn test_rewrite_api_urls_for_endpoint_base_url_rejects_invalid_endpoint_base_url() {
        let doc = minimal_doc("https://www.googleapis.com/", "drive/v3/", None);

        let err =
            rewrite_api_urls_for_endpoint_base_url(doc, Some("proxy.example.com")).unwrap_err();

        assert!(err
            .to_string()
            .contains("Invalid GOOGLE_WORKSPACE_CLI_API_ENDPOINT_BASE_URL"));
    }

    #[test]
    fn test_rewrite_api_urls_for_endpoint_base_url_rejects_non_http_scheme() {
        let doc = minimal_doc("https://www.googleapis.com/", "drive/v3/", None);

        let err = rewrite_api_urls_for_endpoint_base_url(doc, Some("ftp://proxy.example.com"))
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("GOOGLE_WORKSPACE_CLI_API_ENDPOINT_BASE_URL must use http or https"));
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn test_fetch_discovery_document_rewrites_cache_hit_without_mutating_cache() {
        let _env_guard = EnvGuard::set(API_ENDPOINT_BASE_URL_ENV, "https://proxy.example.com/");
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_file = cache_dir.path().join("drive_v3.json");
        let cached_json = r#"{
            "name": "drive",
            "version": "v3",
            "rootUrl": "https://www.googleapis.com/",
            "servicePath": "drive/v3/",
            "baseUrl": "https://www.googleapis.com/drive/v3/"
        }"#;
        std::fs::write(&cache_file, cached_json).unwrap();

        let doc = fetch_discovery_document("drive", "v3", Some(cache_dir.path()))
            .await
            .unwrap();

        assert_eq!(doc.root_url, "https://proxy.example.com/");
        assert_eq!(doc.service_path, "drive/v3/");
        assert_eq!(
            doc.base_url.as_deref(),
            Some("https://proxy.example.com/drive/v3/")
        );
        assert_eq!(std::fs::read_to_string(&cache_file).unwrap(), cached_json);
    }

    #[test]
    #[serial_test::serial]
    fn test_api_endpoint_base_url_from_env_handles_missing_empty_and_configured_values() {
        let missing_guard = EnvGuard::remove(API_ENDPOINT_BASE_URL_ENV);
        assert_eq!(api_endpoint_base_url_from_env(), None);
        drop(missing_guard);

        let empty_guard = EnvGuard::set(API_ENDPOINT_BASE_URL_ENV, "  ");
        assert_eq!(api_endpoint_base_url_from_env(), None);
        drop(empty_guard);

        let value_guard = EnvGuard::set(API_ENDPOINT_BASE_URL_ENV, " https://proxy.example.com/ ");
        assert_eq!(
            api_endpoint_base_url_from_env().as_deref(),
            Some("https://proxy.example.com/")
        );
        drop(value_guard);
    }
}

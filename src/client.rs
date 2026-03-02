use reqwest::header::{HeaderMap, HeaderValue};

pub fn build_client() -> Result<reqwest::Client, crate::error::GwsError> {
    let mut headers = HeaderMap::new();
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");

    // Format: name/version
    let client_header = format!("{}/{}", name, version);
    if let Ok(header_value) = HeaderValue::from_str(&client_header) {
        headers.insert("x-goog-api-client", header_value);
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| {
            crate::error::GwsError::Other(anyhow::anyhow!("Failed to build HTTP client: {e}"))
        })
}

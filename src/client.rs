use reqwest::header::{HeaderMap, HeaderValue};

pub fn build_client() -> Result<reqwest::Client, crate::error::GwsError> {
    let mut headers = HeaderMap::new();
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");

    // Format: gl-rust/name-version (the gl-rust/ prefix is fixed)
    let client_header = format!("gl-rust/{}-{}", name, version);
    if let Ok(header_value) = HeaderValue::from_str(&client_header) {
        headers.insert("x-goog-api-client", header_value);
    }

    // Set quota project from ADC for billing/quota attribution
    if let Some(quota_project) = crate::auth::get_quota_project() {
        if let Ok(header_value) = HeaderValue::from_str(&quota_project) {
            headers.insert("x-goog-user-project", header_value);
        }
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| {
            crate::error::GwsError::Other(anyhow::anyhow!("Failed to build HTTP client: {e}"))
        })
}

const MAX_RETRIES: u32 = 3;

/// Send an HTTP request with automatic retry on 429 (rate limit) responses.
/// Respects the `Retry-After` header; falls back to exponential backoff (1s, 2s, 4s).
pub async fn send_with_retry(
    build_request: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, reqwest::Error> {
    for attempt in 0..MAX_RETRIES {
        let resp = build_request().send().await?;

        if resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Ok(resp);
        }

        // Parse Retry-After header (seconds), fall back to exponential backoff
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(1 << attempt); // 1, 2, 4 seconds

        tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
    }

    // Final attempt — return whatever we get
    build_request().send().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_client_succeeds() {
        assert!(build_client().is_ok());
    }
}

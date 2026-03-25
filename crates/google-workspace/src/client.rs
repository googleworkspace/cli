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

//! HTTP client with retry logic for Google API requests.

use std::sync::OnceLock;

use reqwest::header::{HeaderMap, HeaderValue};

const MAX_RETRIES: u32 = 3;
/// Maximum seconds to sleep on a 429 Retry-After header. Prevents a hostile
/// or misconfigured server from hanging the process indefinitely.
const MAX_RETRY_DELAY_SECS: u64 = 60;
const CONNECT_TIMEOUT_SECS: u64 = 10;

fn build_client_inner() -> Result<reqwest::Client, String> {
    let mut headers = HeaderMap::new();
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");

    // Format: gl-rust/name-version (the gl-rust/ prefix is fixed)
    let client_header = format!("gl-rust/{}-{}", name, version);
    if let Ok(header_value) = HeaderValue::from_str(&client_header) {
        headers.insert("x-goog-api-client", header_value);
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

pub fn build_client() -> Result<reqwest::Client, crate::error::GwsError> {
    build_client_inner().map_err(|message| crate::error::GwsError::Other(anyhow::anyhow!(message)))
}

/// Returns a shared reqwest client clone backed by a single global connection pool.
///
/// `reqwest::Client` is cheap to clone, so callers can take ownership of the
/// returned value while still sharing pooled connections underneath.
pub fn shared_client() -> Result<reqwest::Client, crate::error::GwsError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();

    match CLIENT.get_or_init(build_client_inner) {
        Ok(client) => Ok(client.clone()),
        Err(message) => Err(crate::error::GwsError::Other(anyhow::anyhow!(
            message.clone()
        ))),
    }
}

/// Send an HTTP request with automatic retry on 429 (rate limit) responses
/// and transient connection/timeout errors.
/// Respects the `Retry-After` header; falls back to exponential backoff (1s, 2s, 4s).
pub async fn send_with_retry(
    build_request: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut last_err: Option<reqwest::Error> = None;

    for attempt in 0..MAX_RETRIES {
        match build_request().send().await {
            Ok(resp) => {
                if resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
                    return Ok(resp);
                }

                let header_value = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok());
                let retry_after = compute_retry_delay(header_value, attempt);
                tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
            }
            Err(e) if e.is_connect() || e.is_timeout() => {
                // Transient network error — retry with exponential backoff
                let delay = compute_retry_delay(None, attempt);
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }

    // Final attempt — return whatever we get
    match build_request().send().await {
        Ok(resp) => Ok(resp),
        Err(e) => Err(last_err.unwrap_or(e)),
    }
}

/// Compute the retry delay from a Retry-After header value and attempt number.
/// Falls back to exponential backoff (1, 2, 4s) when the header is absent or
/// unparseable. Always caps the result at MAX_RETRY_DELAY_SECS.
fn compute_retry_delay(header_value: Option<&str>, attempt: u32) -> u64 {
    header_value
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(2u64.saturating_pow(attempt))
        .min(MAX_RETRY_DELAY_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_client_succeeds() {
        assert!(build_client().is_ok());
    }

    #[test]
    fn shared_client_succeeds() {
        assert!(shared_client().is_ok());
    }

    #[test]
    fn shared_client_can_be_reused() {
        let client_a = shared_client().unwrap();
        let client_b = shared_client().unwrap();
        let request_a = client_a.get("https://example.com").build().unwrap();
        let request_b = client_b.get("https://example.com").build().unwrap();
        assert_eq!(request_a.url(), request_b.url());
    }

    #[test]
    fn retry_delay_caps_large_header_value() {
        assert_eq!(compute_retry_delay(Some("999999"), 0), MAX_RETRY_DELAY_SECS);
    }

    #[test]
    fn retry_delay_passes_through_small_header_value() {
        assert_eq!(compute_retry_delay(Some("5"), 0), 5);
    }

    #[test]
    fn retry_delay_falls_back_to_exponential_on_missing_header() {
        assert_eq!(compute_retry_delay(None, 0), 1); // 2^0
        assert_eq!(compute_retry_delay(None, 1), 2); // 2^1
        assert_eq!(compute_retry_delay(None, 2), 4); // 2^2
    }

    #[test]
    fn retry_delay_falls_back_on_unparseable_header() {
        assert_eq!(compute_retry_delay(Some("not-a-number"), 1), 2);
        assert_eq!(compute_retry_delay(Some(""), 0), 1);
    }

    #[test]
    fn retry_delay_caps_at_boundary() {
        assert_eq!(compute_retry_delay(Some("60"), 0), 60);
        assert_eq!(compute_retry_delay(Some("61"), 0), MAX_RETRY_DELAY_SECS);
    }
}

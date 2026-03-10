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

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| {
            crate::error::GwsError::Other(anyhow::anyhow!("Failed to build HTTP client: {e}"))
        })
}

const MAX_RETRIES: u32 = 3;
const MAX_RETRY_AFTER_SECS: u64 = 30;

fn retry_delay_secs(headers: &reqwest::header::HeaderMap, attempt: u32) -> u64 {
    let from_header = headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let backoff = 1u64 << attempt; // 1, 2, 4 seconds
    from_header.unwrap_or(backoff).min(MAX_RETRY_AFTER_SECS)
}

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

        // Parse Retry-After (seconds), fall back to exponential backoff.
        // Clamp to avoid unbounded server-controlled sleep.
        let retry_after = retry_delay_secs(resp.headers(), attempt);

        tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
    }

    // Final attempt — return whatever we get
    build_request().send().await
}

/// Send an already-built request with retry on 429 when the request can be
/// safely cloned for subsequent attempts.
///
/// If the request cannot be cloned (e.g. streaming body), this falls back to a
/// single send.
pub async fn send_builder_with_retry(
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, reqwest::Error> {
    let Some(template) = request.try_clone() else {
        return request.send().await;
    };

    for attempt in 0..MAX_RETRIES {
        // `template` came from `try_clone()`, so this should remain cloneable.
        // If clone unexpectedly fails, degrade safely to a single send.
        let attempt_request = match template.try_clone() {
            Some(r) => r,
            None => return template.send().await,
        };

        let resp = attempt_request.send().await?;

        if resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Ok(resp);
        }

        let retry_after = retry_delay_secs(resp.headers(), attempt);

        tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
    }

    match template.try_clone() {
        Some(r) => r.send().await,
        None => template.send().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn reason_phrase(code: u16) -> &'static str {
        match code {
            200 => "OK",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Status",
        }
    }

    async fn spawn_response_server(
        responses: Vec<(u16, Option<u64>)>,
    ) -> (
        String,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<Result<(), std::io::Error>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_clone = Arc::clone(&hits);

        let handle = tokio::spawn(async move {
            for (status, retry_after) in responses {
                let (mut socket, _) = listener.accept().await?;
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await?;
                hits_clone.fetch_add(1, Ordering::SeqCst);

                let body = b"{}";
                let mut extra_headers = String::new();
                if let Some(v) = retry_after {
                    extra_headers.push_str(&format!("Retry-After: {v}\r\n"));
                }

                let response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n",
                    status,
                    reason_phrase(status),
                    body.len(),
                    extra_headers
                );
                socket.write_all(response.as_bytes()).await?;
                socket.write_all(body).await?;
            }
            Ok(())
        });

        (format!("http://{addr}/"), hits, handle)
    }

    #[test]
    fn build_client_succeeds() {
        assert!(build_client().is_ok());
    }

    #[test]
    fn retry_delay_secs_clamps_large_retry_after() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("9999"));
        assert_eq!(retry_delay_secs(&headers, 0), MAX_RETRY_AFTER_SECS);
    }

    #[test]
    fn retry_delay_secs_uses_exponential_fallback() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(retry_delay_secs(&headers, 0), 1);
        assert_eq!(retry_delay_secs(&headers, 1), 2);
        assert_eq!(retry_delay_secs(&headers, 2), 4);
    }

    #[tokio::test]
    async fn send_with_retry_retries_on_429() {
        let (url, hits, handle) = spawn_response_server(vec![(429, Some(0)), (200, None)]).await;
        let client = reqwest::Client::new();

        let resp = send_with_retry(|| client.get(&url)).await.unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn send_builder_with_retry_retries_on_429() {
        let (url, hits, handle) = spawn_response_server(vec![(429, Some(0)), (200, None)]).await;
        let client = reqwest::Client::new();
        let request = client.get(&url).header("x-test", "1");

        let resp = send_builder_with_retry(request).await.unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn send_builder_with_retry_does_not_retry_non_429() {
        let (url, hits, handle) = spawn_response_server(vec![(500, None)]).await;
        let client = reqwest::Client::new();
        let request = client.get(&url);

        let resp = send_builder_with_retry(request).await.unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        handle.await.unwrap().unwrap();
    }
}

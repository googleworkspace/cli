use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(crate) struct MockHttpResponse {
    pub status: u16,
    pub retry_after_secs: Option<u64>,
    pub body: &'static str,
}

pub(crate) fn mock_http_response(
    status: u16,
    retry_after_secs: Option<u64>,
    body: &'static str,
) -> MockHttpResponse {
    MockHttpResponse {
        status,
        retry_after_secs,
        body,
    }
}

fn reason_phrase(code: u16) -> &'static str {
    match code {
        200 => "OK",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

pub(crate) async fn spawn_response_server(
    responses: Vec<MockHttpResponse>,
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
        for MockHttpResponse {
            status,
            retry_after_secs,
            body,
        } in responses
        {
            let (mut socket, _) = listener.accept().await?;
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).await?;
            hits_clone.fetch_add(1, Ordering::SeqCst);

            let mut extra_headers = String::new();
            if let Some(v) = retry_after_secs {
                extra_headers.push_str(&format!("Retry-After: {v}\r\n"));
            }

            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                status,
                reason_phrase(status),
                body.len(),
                extra_headers,
                body
            );
            socket.write_all(response.as_bytes()).await?;
        }
        Ok(())
    });

    (format!("http://{addr}/"), hits, handle)
}

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

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tempfile::TempDir;

const BODY: &str =
    r#"{"requests":[{"insertText":{"text":"hello","endOfSegmentLocation":{"segmentId":""}}}]}"#;
const RAW: &[&str] = &[
    "docs",
    "documents",
    "batchUpdate",
    "--params",
    r#"{"documentId":"doc /?#","fields":"documentId"}"#,
    "--json",
    BODY,
];
const WRITE: &[&str] = &["docs", "+write", "--document", "doc /?#", "--text", "hello"];

// Observe every API/proxy connection without contacting Google. Returning an
// error also lets real-request tests verify that API failures stay failures.
struct NetworkTrap {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl NetworkTrap {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let (stop, stopping) = mpsc::channel();
        let worker = thread::spawn(move || loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Record the connection even if the client fails before
                    // sending HTTP (for example, during TLS/proxy setup).
                    let mut requests = captured.lock().unwrap();
                    requests.push(String::new());
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut header = Vec::new();
                    let mut buffer = [0; 1024];
                    while header.len() < 8192 && !header.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut buffer) {
                            Ok(0) | Err(_) => break,
                            Ok(size) => header.extend_from_slice(&buffer[..size]),
                        }
                    }
                    *requests.last_mut().unwrap() = String::from_utf8_lossy(&header).into_owned();
                    let body = r#"{"error":{"code":403,"message":"Synthetic denial","errors":[{"reason":"forbidden"}]}}"#;
                    let _ = write!(
                        stream,
                        "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if stopping.recv_timeout(Duration::from_millis(5))
                        != Err(mpsc::RecvTimeoutError::Timeout)
                    {
                        break;
                    }
                }
                Err(error) => panic!("Network trap failed: {error}"),
            }
        });
        Self {
            url,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for NetworkTrap {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        self.worker.take().unwrap().join().unwrap();
    }
}

type Snapshot = BTreeMap<PathBuf, (Vec<u8>, SystemTime)>;

fn snapshot(root: &Path) -> Snapshot {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(snapshot(&path));
        } else {
            files.insert(
                path.clone(),
                (
                    fs::read(&path).unwrap(),
                    fs::metadata(path).unwrap().modified().unwrap(),
                ),
            );
        }
    }
    files
}

struct Fixture {
    dir: TempDir,
    config: PathBuf,
    network: NetworkTrap,
}

impl Fixture {
    fn new(corrupt_credentials: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        fs::create_dir_all(config.join("cache")).unwrap();
        // Stop dotenvy's ancestor search and isolate all credential sources.
        fs::write(dir.path().join(".env"), "").unwrap();
        fs::create_dir(dir.path().join("home")).unwrap();
        let fixture = Self {
            dir,
            config,
            network: NetworkTrap::new(),
        };
        fixture.write_discovery(&fixture.discovery());
        if corrupt_credentials {
            for name in [
                "credentials.enc",
                "credentials.json",
                ".encryption_key",
                "token_cache.json",
                "sa_token_cache.json",
            ] {
                fs::write(
                    fixture.config.join(name),
                    format!("corrupt synthetic sentinel: {name}"),
                )
                .unwrap();
            }
        }
        fixture
    }

    fn discovery(&self) -> Value {
        json!({
            "name": "docs",
            "version": "v1",
            "rootUrl": self.network.url,
            "servicePath": "v1/",
            "resources": {
                "documents": {
                    "methods": {
                        "batchUpdate": {
                            "id": "docs.documents.batchUpdate",
                            "httpMethod": "POST",
                            "path": "documents/{documentId}:batchUpdate",
                            "parameterOrder": ["documentId"],
                            "parameters": {
                                "documentId": {"type": "string", "location": "path", "required": true},
                                "fields": {"type": "string", "location": "query"}
                            },
                            "request": {"$ref": "BatchUpdateDocumentRequest"},
                            "scopes": ["https://www.googleapis.com/auth/documents"]
                        }
                    }
                }
            },
            "schemas": {
                "BatchUpdateDocumentRequest": {
                    "type": "object",
                    "required": ["requests"],
                    "properties": {
                        "requests": {"type": "array", "items": {"$ref": "Request"}}
                    }
                },
                "Request": {
                    "type": "object",
                    "properties": {
                        "insertText": {
                            "type": "object",
                            "properties": {
                                "text": {"type": "string"},
                                "endOfSegmentLocation": {
                                    "type": "object",
                                    "properties": {"segmentId": {"type": "string"}}
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    fn write_discovery(&self, discovery: &Value) {
        // Exercise the real config override, cache filename and freshness check.
        fs::write(
            self.config.join("cache/docs_v1.json"),
            serde_json::to_vec(discovery).unwrap(),
        )
        .unwrap();
    }

    fn run(&self, args: &[&str], token: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gws"));
        command
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("HOME", self.dir.path().join("home"))
            .env("USERPROFILE", self.dir.path().join("home"))
            .env("APPDATA", self.dir.path().join("home"))
            .env("XDG_CONFIG_HOME", self.dir.path().join("home"))
            .env("USER", "gws-dry-run-test")
            .env("USERNAME", "gws-dry-run-test")
            .env("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", &self.config)
            // Never query an actual OS account, even when testing broken auth.
            .env("GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND", "file")
            .env("HTTP_PROXY", &self.network.url)
            .env("HTTPS_PROXY", &self.network.url)
            .env("ALL_PROXY", &self.network.url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        if let Some(token) = token {
            command.env("GOOGLE_WORKSPACE_CLI_TOKEN", token);
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("CLI timed out: {output:?}");
            }
            thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }

    fn dry_run(&self, args: &[&str], exit_code: i32) -> Value {
        let before = snapshot(self.dir.path());
        let mut args = args.to_vec();
        args.push("--dry-run");
        let output = self.run(&args, None);
        assert_eq!(
            output.status.code(),
            Some(exit_code),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let after = snapshot(self.dir.path());
        assert_eq!(
            after.keys().collect::<Vec<_>>(),
            before.keys().collect::<Vec<_>>(),
            "Dry-run created or deleted fixture files"
        );
        for (path, expected) in &before {
            assert!(
                after.get(path) == Some(expected),
                "Dry-run changed contents or mtime: {path:?}"
            );
        }
        assert!(
            self.network.requests.lock().unwrap().is_empty(),
            "Dry-run attempted an API, auth or Discovery connection"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("keyring") && !stderr.contains("credentials"),
            "Dry-run unexpectedly used auth: {stderr}"
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn assert_preview(&self, preview: &Value, query: Value) {
        assert_eq!(
            preview,
            &json!({
                "dry_run": true,
                "url": format!("{}v1/documents/doc%20%2F%3F%23:batchUpdate", self.network.url),
                "method": "POST",
                "query_params": query,
                "body": {
                    "requests": [{
                        "insertText": {
                            "text": "hello",
                            "endOfSegmentLocation": {"segmentId": ""}
                        }
                    }]
                },
                "is_multipart_upload": false
            })
        );
    }
}

#[test]
fn raw_dry_run_preserves_corrupt_credentials() {
    let fixture = Fixture::new(true);
    fixture.assert_preview(&fixture.dry_run(RAW, 0), json!([["fields", "documentId"]]));
}

#[test]
fn raw_dry_run_needs_no_credentials() {
    let fixture = Fixture::new(false);
    fixture.assert_preview(&fixture.dry_run(RAW, 0), json!([["fields", "documentId"]]));
}

#[test]
fn docs_write_dry_run_preserves_corrupt_credentials() {
    let fixture = Fixture::new(true);
    fixture.assert_preview(&fixture.dry_run(WRITE, 0), json!([]));
}

#[test]
fn docs_write_dry_run_needs_no_credentials() {
    let fixture = Fixture::new(false);
    fixture.assert_preview(&fixture.dry_run(WRITE, 0), json!([]));
}

fn assert_validation(error: &Value, message: &str) {
    assert_eq!(error["error"]["reason"], "validationError");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains(message),
        "{error}"
    );
}

#[test]
fn raw_dry_run_rejects_malformed_body_without_auth() {
    let fixture = Fixture::new(true);
    let mut args = RAW.to_vec();
    *args.last_mut().unwrap() = "{";
    assert_validation(&fixture.dry_run(&args, 3), "Invalid --json body");
}

#[test]
fn raw_dry_run_validates_nested_body_without_auth() {
    let fixture = Fixture::new(true);
    let mut args = RAW.to_vec();
    *args.last_mut().unwrap() = r#"{"requests":[{"insertText":{"text":42}}]}"#;
    assert_validation(&fixture.dry_run(&args, 3), "Expected type 'string'");
}

#[test]
fn raw_dry_run_rejects_malformed_params_without_auth() {
    let fixture = Fixture::new(true);
    let mut args = RAW.to_vec();
    args[4] = "{";
    assert_validation(&fixture.dry_run(&args, 3), "Invalid --params JSON");
}

#[test]
fn raw_dry_run_requires_path_parameter_without_auth() {
    let fixture = Fixture::new(true);
    let mut args = RAW.to_vec();
    args[4] = "{}";
    assert_validation(&fixture.dry_run(&args, 3), "documentId is missing");
}

#[test]
fn raw_dry_run_requires_query_parameter_without_auth() {
    let fixture = Fixture::new(true);
    let mut doc = fixture.discovery();
    doc["resources"]["documents"]["methods"]["batchUpdate"]["parameters"]["revision"] =
        json!({"type": "string", "location": "query", "required": true});
    fixture.write_discovery(&doc);
    assert_validation(&fixture.dry_run(RAW, 3), "'revision' is missing");
}

#[test]
fn raw_dry_run_rejects_resource_traversal_without_auth() {
    let fixture = Fixture::new(true);
    let mut doc = fixture.discovery();
    doc["resources"]["documents"]["methods"]["batchUpdate"]["path"] =
        json!("documents/{+documentId}:batchUpdate");
    fixture.write_discovery(&doc);
    let mut args = RAW.to_vec();
    args[4] = r#"{"documentId":"../outside"}"#;
    assert_validation(&fixture.dry_run(&args, 3), "traversal");
}

#[test]
fn raw_dry_run_rejects_output_traversal_without_auth() {
    let fixture = Fixture::new(true);
    let mut args = RAW.to_vec();
    args.extend(["--output", "../outside"]);
    assert_validation(&fixture.dry_run(&args, 3), "outside the current directory");
}

#[test]
fn docs_write_dry_run_requires_document() {
    let fixture = Fixture::new(true);
    assert_validation(
        &fixture.dry_run(&["docs", "+write", "--text", "hello"], 3),
        "--document",
    );
}

#[test]
fn docs_write_dry_run_requires_text() {
    let fixture = Fixture::new(true);
    assert_validation(
        &fixture.dry_run(&["docs", "+write", "--document", "doc"], 3),
        "--text",
    );
}

#[test]
fn docs_write_dry_run_validates_generated_body_without_auth() {
    let fixture = Fixture::new(true);
    let mut doc = fixture.discovery();
    doc["schemas"]["BatchUpdateDocumentRequest"]["required"] = json!(["requests", "title"]);
    fixture.write_discovery(&doc);
    assert_validation(
        &fixture.dry_run(WRITE, 3),
        "Missing required property 'title'",
    );
}

#[test]
fn docs_write_dry_run_preserves_discovery_errors() {
    let fixture = Fixture::new(true);
    let mut doc = fixture.discovery();
    doc["resources"]["documents"]["methods"] = json!({});
    fixture.write_discovery(&doc);
    let error = fixture.dry_run(WRITE, 4);
    assert_eq!(error["error"]["reason"], "discoveryError");
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("batchUpdate"));
}

fn assert_auth_failure(args: &[&str]) {
    let fixture = Fixture::new(true);
    let output = fixture.run(args, None);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["reason"], "authError");
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("credentials"));
    // Prove this is the real auth path: existing corrupt-credential cleanup
    // still runs, and the broken plaintext fallback remains a hard error.
    assert!(!fixture.config.join("credentials.enc").exists());
    assert!(!fixture.config.join("token_cache.json").exists());
    assert!(!fixture.config.join("sa_token_cache.json").exists());
    assert!(fixture.network.requests.lock().unwrap().is_empty());
}

#[test]
fn raw_real_request_still_fails_on_broken_credentials() {
    assert_auth_failure(RAW);
}

#[test]
fn docs_write_real_request_still_fails_on_broken_credentials() {
    assert_auth_failure(WRITE);
}

#[test]
fn raw_real_request_without_credentials_preserves_access_denied() {
    let fixture = Fixture::new(false);
    let output = fixture.run(RAW, None);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["reason"], "authError");
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No credentials provided"));
    let requests = fixture.network.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].to_lowercase().contains("authorization:"));
}

fn assert_authenticated_api_failure(args: &[&str]) {
    let fixture = Fixture::new(false);
    let output = fixture.run(args, Some("synthetic-test-token"));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], 403);
    assert_eq!(error["error"]["message"], "Synthetic denial");
    let requests = fixture.network.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .to_lowercase()
        .contains("authorization: bearer synthetic-test-token"));
}

#[test]
fn raw_real_request_uses_token_and_preserves_api_failure() {
    assert_authenticated_api_failure(RAW);
}

#[test]
fn docs_write_real_request_uses_token_and_preserves_api_failure() {
    assert_authenticated_api_failure(WRITE);
}

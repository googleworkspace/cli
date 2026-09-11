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

//! Exercise the real CLI with synthetic cached Discovery and child-only env.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::{tempdir, TempDir};

struct Fixture {
    _temp: TempDir,
    cwd: PathBuf,
    root: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new(root_url: &str) -> Self {
        let temp = tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let cwd = base.join("working");
        let root = base.join("files");
        let config = base.join("config");
        fs::create_dir(&cwd).unwrap();
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(config.join("cache")).unwrap();
        // Stop dotenv from searching parent directories for real configuration.
        fs::write(cwd.join(".env"), "").unwrap();
        fs::write(config.join("cache/drive_v3.json"), json!({
            "name": "drive", "version": "v3", "rootUrl": root_url,
            "resources": {"files": {"methods": {
                "get": {"httpMethod": "GET", "path": "files/synthetic"},
                "create": {"httpMethod": "POST", "path": "files", "supportsMediaUpload": true,
                    "mediaUpload": {"protocols": {"simple": {"path": "/upload/files", "multipart": true}}}}
            }}}
        }).to_string()).unwrap();
        Self {
            _temp: temp,
            cwd,
            root,
            config,
        }
    }

    fn command(&self, configured: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gws"));
        command
            .env_clear()
            .current_dir(&self.cwd)
            .env("HOME", &self.cwd)
            .env("USERPROFILE", &self.cwd)
            .env("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", &self.config)
            .env("GOOGLE_WORKSPACE_CLI_TOKEN", "synthetic-test-token")
            .env("GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND", "file")
            .env("NO_COLOR", "1")
            // A cache regression must fail locally, never fetch real Discovery.
            .env("HTTPS_PROXY", "http://127.0.0.1:1")
            .env("HTTP_PROXY", "http://127.0.0.1:1")
            .env("NO_PROXY", "127.0.0.1,localhost");
        if configured {
            command.env("GOOGLE_WORKSPACE_CLI_FILE_ROOT", &self.root);
        }
        command
    }

    fn dry_run(&self, flag: &str, path: &Path, configured: bool) -> Output {
        let method = if flag == "--upload" { "create" } else { "get" };
        self.command(configured)
            .args(["drive", "files", method, "--dry-run", flag])
            .arg(path)
            .output()
            .unwrap()
    }
}

fn successful_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "status: {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn file_root_cli_dry_run_accepts_external_output_and_upload() {
    let fixture = Fixture::new("http://127.0.0.1:1/");
    fs::write(fixture.root.join("upload.txt"), "synthetic upload").unwrap();
    for (flag, name) in [("--output", "new.bin"), ("--upload", "upload.txt")] {
        let output = fixture.dry_run(flag, &fixture.root.join(name), true);
        let result = successful_json(&output);
        assert_eq!(result["dry_run"], true);
        assert_eq!(result["is_multipart_upload"], flag == "--upload");
    }
    assert!(!fixture.root.join("new.bin").exists());
}

#[test]
fn file_root_cli_default_rejects_external_paths_but_keeps_local_output() {
    let fixture = Fixture::new("http://127.0.0.1:1/");
    let output = fixture.dry_run("--output", &fixture.root.join("new.bin"), false);
    assert_eq!(output.status.code(), Some(3));
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("current directory"));
    assert_eq!(
        successful_json(&fixture.dry_run("--output", Path::new("new.bin"), false))["dry_run"],
        true
    );
}

#[test]
fn file_root_cli_rejects_cwd_relative_path_outside_configured_root() {
    let fixture = Fixture::new("http://127.0.0.1:1/");
    let output = fixture.dry_run("--output", Path::new("new.bin"), true);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("GOOGLE_WORKSPACE_CLI_FILE_ROOT"));
}

#[test]
fn file_root_cli_download_propagates_canonical_output() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let fixture = Fixture::new(&format!("http://{}/", listener.local_addr().unwrap()));
    let output_path = fixture.root.join("./output.bin");
    let mut child = fixture
        .command(true)
        .args(["drive", "files", "get", "--output"])
        .arg(&output_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    assert!(read > 0, "request ended before headers");
                    request.extend_from_slice(&buffer[..read]);
                }
                assert!(request.starts_with(b"GET /files/synthetic HTTP/1.1\r\n"));
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 9\r\nConnection: close\r\n\r\nsynthetic").unwrap();
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    let output = child.wait_with_output().unwrap();
                    panic!(
                        "CLI did not contact local fixture: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("local fixture failed: {err}"),
        }
    }
    let result = successful_json(&child.wait_with_output().unwrap());
    assert_eq!(
        result["saved_file"],
        fixture.root.join("output.bin").to_str().unwrap()
    );
    assert_eq!(result["bytes"], 9);
    assert_eq!(
        fs::read(fixture.root.join("output.bin")).unwrap(),
        b"synthetic"
    );
    assert!(!fixture.cwd.join("output.bin").exists());
}

// macOS filesystems commonly reject invalid UTF-8 directory names. The CLI
// conversion itself is covered without filesystem access by main.rs unit tests;
// these Linux regressions exercise the complete canonical symlink handoff.
#[cfg(target_os = "linux")]
fn non_utf8_alias(fixture: &Fixture, filename: &str) -> PathBuf {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let target = fixture
        .root
        .join(std::ffi::OsString::from_vec(b"bytes-\xff".to_vec()));
    fs::create_dir(&target).unwrap();
    fs::write(target.join("upload.txt"), b"synthetic upload").unwrap();
    let alias = fixture.root.join("alias");
    symlink(&target, &alias).unwrap();
    alias.join(filename)
}

#[cfg(target_os = "linux")]
#[test]
fn file_root_cli_rejects_non_utf8_output_without_fallback_write() {
    let fixture = Fixture::new("http://127.0.0.1:1/");
    let output_path = non_utf8_alias(&fixture, "new.bin");
    let fallback = fixture.cwd.join("download.bin");
    fs::write(&fallback, b"keep existing download").unwrap();
    // A non-dry run must fail at validation, before HTTP or the default output
    // can be selected. No live service or credentials are involved.
    let output = fixture
        .command(true)
        .args(["drive", "files", "get", "--output"])
        .arg(&output_path)
        .output()
        .unwrap();
    assert_eq!(fs::read(&fallback).unwrap(), b"keep existing download");
    assert!(!output_path.exists());
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    let message = error["error"]["message"].as_str().unwrap();
    assert!(message.contains("--output"), "{message}");
    assert!(message.contains("UTF-8"), "{message}");
}

#[cfg(target_os = "linux")]
#[test]
fn file_root_cli_rejects_non_utf8_upload_instead_of_omitting_it() {
    let fixture = Fixture::new("http://127.0.0.1:1/");
    let upload_path = non_utf8_alias(&fixture, "upload.txt");
    let output = fixture.dry_run("--upload", &upload_path, true);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    let message = error["error"]["message"].as_str().unwrap();
    assert!(message.contains("--upload"), "{message}");
    assert!(message.contains("UTF-8"), "{message}");
    assert_eq!(fs::read(&upload_path).unwrap(), b"synthetic upload");
}

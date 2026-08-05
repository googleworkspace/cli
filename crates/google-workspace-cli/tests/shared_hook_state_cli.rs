use base64::Engine;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;

const CONTRACT_RELATIVE_PATH: &str = "docs/governance/shared-hook-state.v1.json";
const PRE_COMMIT_BYTES: &[u8] = b"precommit-authority\n";
const PRE_COMMIT_SHA256: &str = "bb3b9766964cfe4b473b36650b5fdefc016c308b9c3eb5d4aef9bc6691f8e6f3";
const CHECKSUM_BYTES: &[u8] = b"checksum-authority\n";
const CHECKSUM_SHA256: &str = "1b242e7759d9174dc49e0b2df7858f2e375b29bbed5d21d2673d3871c633d181";

fn present_expected(sha256: &str, mode: &str, bytes: &[u8]) -> Value {
    json!({
        "state": "present",
        "sha256": sha256,
        "mode": mode,
        "artifact": {
            "encoding": "base64",
            "content": base64::engine::general_purpose::STANDARD.encode(bytes),
        },
    })
}

fn absent_expected() -> Value {
    json!({ "state": "absent" })
}

fn target(name: &str, relative_path: &str, expected: Value) -> Value {
    json!({
        "name": name,
        "relativePath": relative_path,
        "expected": expected,
    })
}

fn decided_contract(pre_commit: Value, pre_push: Value, checksum: Value) -> Value {
    json!({
        "schema": "shared_hook_state_authority_v1",
        "schemaVersion": 1,
        "authority": {
            "status": "DECIDED",
            "decisionId": "fixture-decision-001",
            "decisionSource": "repository",
            "basis": "task-local fixture decision",
            "mutationAllowed": false,
        },
        "targets": [
            target("pre-commit", ".git/hooks/pre-commit", pre_commit),
            target("pre-push", ".git/hooks/pre-push", pre_push),
            target("lefthook.checksum", ".git/info/lefthook.checksum", checksum),
        ],
    })
}

fn proposed_contract() -> Value {
    json!({
        "schema": "shared_hook_state_authority_v1",
        "schemaVersion": 1,
        "authority": {
            "status": "PROPOSED",
            "decisionId": null,
            "decisionSource": "repository",
            "basis": "no exact preimage decision is active",
            "mutationAllowed": false,
        },
        "targets": [
            target("pre-commit", ".git/hooks/pre-commit", Value::Null),
            target("pre-push", ".git/hooks/pre-push", Value::Null),
            target("lefthook.checksum", ".git/info/lefthook.checksum", Value::Null),
        ],
    })
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("fixture tempdir");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(dir.path())
        .status()
        .expect("git init should execute");
    assert!(status.success());
    fs::create_dir_all(dir.path().join("docs/governance")).expect("fixture contract directory");
    dir
}

fn write_contract(root: &Path, contract: &Value) {
    fs::write(
        root.join(CONTRACT_RELATIVE_PATH),
        serde_json::to_vec_pretty(contract).expect("fixture contract JSON"),
    )
    .expect("write fixture contract");
}

fn write_raw_contract(root: &Path, contents: &str) {
    fs::write(root.join(CONTRACT_RELATIVE_PATH), contents).expect("write raw fixture contract");
}

fn run_fixture(root: &Path) -> (Value, i32) {
    run_fixture_args(root, &["shared-hook-state"])
}

fn run_fixture_args(root: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_gws"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("the gws binary should execute");
    let result: Value = serde_json::from_slice(&output.stdout)
        .expect("shared-hook-state should emit structured JSON on stdout");
    (result, output.status.code().expect("stable exit code"))
}

fn write_target(root: &Path, relative_path: &str, bytes: &[u8], mode: u32) {
    let path = root.join(relative_path);
    fs::create_dir_all(path.parent().expect("target parent")).expect("target parent");
    fs::write(&path, bytes).expect("write fixture target");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .expect("set fixture target mode");
    }
}

fn drift_kinds(result: &Value) -> Vec<&str> {
    result["drift"]["items"]
        .as_array()
        .expect("drift items array")
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect()
}

#[test]
fn shared_hook_state_is_structured_and_fail_closed_when_proposal_is_active() {
    let output = Command::new(env!("CARGO_BIN_EXE_gws"))
        .arg("shared-hook-state")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the gws binary should execute");

    let result: Value = serde_json::from_slice(&output.stdout)
        .expect("shared-hook-state should emit structured JSON on stdout");

    assert_eq!(result["schema"], "shared_hook_state_verification_result_v1");
    assert_eq!(result["authorityStatus"], "PROPOSED");
    assert_eq!(result["failClosed"], true);
    assert_eq!(output.status.code(), Some(20));
}

#[test]
fn fixture_proposal_blocks_without_inventing_expected_states() {
    let dir = fixture();
    write_contract(dir.path(), &proposed_contract());

    let (result, code) = run_fixture(dir.path());

    assert_eq!(code, 20);
    assert_eq!(result["authorityStatus"], "PROPOSED");
    assert_eq!(result["failClosed"], true);
    assert!(result["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .all(|target| target["expected"].is_null()));
}

#[test]
fn fixture_decision_matches_exact_present_and_absent_targets() {
    let dir = fixture();
    write_target(dir.path(), ".git/hooks/pre-commit", PRE_COMMIT_BYTES, 0o755);
    write_target(
        dir.path(),
        ".git/info/lefthook.checksum",
        CHECKSUM_BYTES,
        0o644,
    );
    write_contract(
        dir.path(),
        &decided_contract(
            present_expected(PRE_COMMIT_SHA256, "0755", PRE_COMMIT_BYTES),
            absent_expected(),
            present_expected(CHECKSUM_SHA256, "0644", CHECKSUM_BYTES),
        ),
    );

    let (result, code) = run_fixture(dir.path());

    assert_eq!(code, 0);
    assert_eq!(result["status"], "PASS");
    assert_eq!(result["authorityStatus"], "DECIDED");
    assert_eq!(result["failClosed"], false);
    assert_eq!(result["drift"]["status"], "none");
    assert_eq!(result["targets"][0]["observed"]["state"], "present");
    assert_eq!(result["targets"][1]["observed"]["state"], "absent");
    assert_eq!(result["targets"][2]["observed"]["state"], "present");
}

#[test]
fn fixture_hash_mismatch_is_bounded_drift() {
    let dir = fixture();
    write_target(
        dir.path(),
        ".git/hooks/pre-commit",
        b"changed-content\n",
        0o755,
    );
    write_contract(
        dir.path(),
        &decided_contract(
            present_expected(PRE_COMMIT_SHA256, "0755", PRE_COMMIT_BYTES),
            absent_expected(),
            absent_expected(),
        ),
    );

    let (result, code) = run_fixture(dir.path());

    assert_eq!(code, 23);
    assert_eq!(result["status"], "DRIFT");
    assert!(drift_kinds(&result).contains(&"hash_mismatch"));
}

#[test]
fn fixture_mode_mismatch_is_bounded_drift() {
    let dir = fixture();
    write_target(dir.path(), ".git/hooks/pre-commit", PRE_COMMIT_BYTES, 0o644);
    write_contract(
        dir.path(),
        &decided_contract(
            present_expected(PRE_COMMIT_SHA256, "0755", PRE_COMMIT_BYTES),
            absent_expected(),
            absent_expected(),
        ),
    );

    let (result, code) = run_fixture(dir.path());

    assert_eq!(code, 23);
    assert!(drift_kinds(&result).contains(&"mode_mismatch"));
}

#[test]
fn fixture_missing_present_target_is_not_accepted_as_absent() {
    let dir = fixture();
    write_contract(
        dir.path(),
        &decided_contract(
            present_expected(PRE_COMMIT_SHA256, "0755", PRE_COMMIT_BYTES),
            absent_expected(),
            absent_expected(),
        ),
    );

    let (result, code) = run_fixture(dir.path());

    assert_eq!(code, 23);
    assert_eq!(result["targets"][0]["observed"]["state"], "absent");
    assert!(drift_kinds(&result).contains(&"missing"));
}

#[test]
fn fixture_symlink_target_is_rejected() {
    let dir = fixture();
    write_target(dir.path(), ".git/hooks/real-hook", PRE_COMMIT_BYTES, 0o755);
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        dir.path().join(".git/hooks/real-hook"),
        dir.path().join(".git/hooks/pre-commit"),
    )
    .expect("create fixture symlink");
    write_contract(
        dir.path(),
        &decided_contract(
            present_expected(PRE_COMMIT_SHA256, "0755", PRE_COMMIT_BYTES),
            absent_expected(),
            absent_expected(),
        ),
    );

    #[cfg(unix)]
    {
        let (result, code) = run_fixture(dir.path());
        assert_eq!(code, 23);
        assert_eq!(result["targets"][0]["observed"]["state"], "symlink");
        assert!(drift_kinds(&result).contains(&"symlink"));
    }
}

#[test]
fn malformed_contract_rejects_unknown_state_extra_target_and_duplicate_target() {
    let dir = fixture();
    let mut unknown = decided_contract(
        json!({ "state": "unknown" }),
        absent_expected(),
        absent_expected(),
    );
    write_contract(dir.path(), &unknown);
    let (result, code) = run_fixture(dir.path());
    assert_eq!(code, 21);
    assert_eq!(result["contractStatus"], "INVALID");
    assert_eq!(result["failClosed"], true);

    unknown["targets"]
        .as_array_mut()
        .expect("targets")
        .push(target("extra", ".git/hooks/extra", Value::Null));
    write_contract(dir.path(), &unknown);
    let (_, code) = run_fixture(dir.path());
    assert_eq!(code, 21);

    let mut duplicate = decided_contract(absent_expected(), absent_expected(), absent_expected());
    duplicate["targets"][1]["name"] = Value::String("pre-commit".to_string());
    write_contract(dir.path(), &duplicate);
    let (_, code) = run_fixture(dir.path());
    assert_eq!(code, 21);
}

#[test]
fn malformed_contract_rejects_duplicate_json_keys() {
    let dir = fixture();
    write_raw_contract(
        dir.path(),
        r#"{"schema":"shared_hook_state_authority_v1","schema":"shared_hook_state_authority_v1","schemaVersion":1,"authority":{"status":"PROPOSED","decisionId":null,"decisionSource":"repository","basis":"fixture","mutationAllowed":false},"targets":[]}"#,
    );

    let (result, code) = run_fixture(dir.path());

    assert_eq!(code, 21);
    assert_eq!(result["contractStatus"], "INVALID");
}

#[test]
fn malformed_contract_rejects_invalid_hash_and_mode() {
    let dir = fixture();
    let invalid_hash = decided_contract(
        present_expected("not-a-sha256", "0755", PRE_COMMIT_BYTES),
        absent_expected(),
        absent_expected(),
    );
    write_contract(dir.path(), &invalid_hash);
    let (_, code) = run_fixture(dir.path());
    assert_eq!(code, 21);

    let invalid_mode = decided_contract(
        present_expected(PRE_COMMIT_SHA256, "9999", PRE_COMMIT_BYTES),
        absent_expected(),
        absent_expected(),
    );
    write_contract(dir.path(), &invalid_mode);
    let (_, code) = run_fixture(dir.path());
    assert_eq!(code, 21);

    let mut invalid_artifact = decided_contract(
        present_expected(PRE_COMMIT_SHA256, "0755", PRE_COMMIT_BYTES),
        absent_expected(),
        absent_expected(),
    );
    invalid_artifact["targets"][0]["expected"]["artifact"]["content"] =
        Value::String(base64::engine::general_purpose::STANDARD.encode(b"wrong-artifact\n"));
    write_contract(dir.path(), &invalid_artifact);
    let (_, code) = run_fixture(dir.path());
    assert_eq!(code, 21);
}

#[test]
fn malformed_contract_rejects_unsafe_path_and_control_character() {
    let dir = fixture();
    let mut unsafe_path = proposed_contract();
    unsafe_path["targets"][0]["relativePath"] = Value::String("../.ssh".to_string());
    write_contract(dir.path(), &unsafe_path);
    let (_, code) = run_fixture(dir.path());
    assert_eq!(code, 21);

    let mut control = proposed_contract();
    control["authority"]["basis"] = Value::String("bad\u{0000}basis".to_string());
    write_contract(dir.path(), &control);
    let (_, code) = run_fixture(dir.path());
    assert_eq!(code, 21);
}

#[test]
fn missing_contract_is_fail_closed_and_undecided() {
    let dir = fixture();

    let (result, code) = run_fixture(dir.path());

    assert_eq!(code, 21);
    assert_eq!(result["authorityStatus"], "UNDECIDED");
    assert_eq!(result["contractStatus"], "MISSING");
    assert_eq!(result["failClosed"], true);
}

#[test]
fn command_rejects_scope_expanding_path_arguments() {
    let dir = fixture();
    write_contract(dir.path(), &proposed_contract());

    let (result, code) =
        run_fixture_args(dir.path(), &["shared-hook-state", "--repo", "../outside"]);

    assert_eq!(code, 25);
    assert_eq!(result["errorCode"], "INPUT_INVALID");
    assert_eq!(result["commonGitDirStatus"], "UNRESOLVED");
}

#[cfg(unix)]
#[test]
fn symlinked_common_git_dir_blocks_resolution() {
    let dir = fixture();
    let real_git_dir = dir.path().join(".git-real");
    fs::rename(dir.path().join(".git"), &real_git_dir).expect("move fixture git dir");
    std::os::unix::fs::symlink(&real_git_dir, dir.path().join(".git"))
        .expect("symlink fixture git dir");

    let (result, code) = run_fixture(dir.path());

    assert_eq!(code, 22);
    assert_eq!(result["commonGitDirStatus"], "UNRESOLVED");
    assert_eq!(result["errorCode"], "UNSAFE_COMMON_GIT_DIR");
}

#[test]
fn verifier_does_not_write_targets_or_change_modes() {
    let dir = fixture();
    write_target(dir.path(), ".git/hooks/pre-commit", PRE_COMMIT_BYTES, 0o755);
    write_contract(
        dir.path(),
        &decided_contract(
            present_expected(PRE_COMMIT_SHA256, "0755", PRE_COMMIT_BYTES),
            absent_expected(),
            absent_expected(),
        ),
    );
    let target_path = dir.path().join(".git/hooks/pre-commit");
    let before_bytes = fs::read(&target_path).expect("read target before verification");
    #[cfg(unix)]
    let before_mode = std::os::unix::fs::PermissionsExt::mode(
        &fs::symlink_metadata(&target_path)
            .expect("stat target before verification")
            .permissions(),
    );

    let (_, code) = run_fixture(dir.path());

    assert_eq!(code, 0);
    assert_eq!(
        fs::read(&target_path).expect("read target after verification"),
        before_bytes
    );
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(
            &fs::symlink_metadata(&target_path)
                .expect("stat target after verification")
                .permissions(),
        ),
        before_mode
    );
}

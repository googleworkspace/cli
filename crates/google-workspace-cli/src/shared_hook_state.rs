//! Read-only, repository-owned verification of the exact shared Git hook state.
//!
//! The command deliberately has no mutation path. Its only filesystem writes
//! are performed by tests in task-local temporary repositories.

use base64::Engine;
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const COMMAND_NAME: &str = "shared-hook-state";

pub const EXIT_PASS: i32 = 0;
pub const EXIT_UNDECIDED: i32 = 20;
pub const EXIT_CONTRACT_INVALID: i32 = 21;
pub const EXIT_GIT_UNRESOLVED: i32 = 22;
pub const EXIT_DRIFT: i32 = 23;
pub const EXIT_OBSERVATION_ERROR: i32 = 24;
pub const EXIT_INPUT_INVALID: i32 = 25;

const CONTRACT_RELATIVE_PATH: &str = "docs/governance/shared-hook-state.v1.json";
const CONTRACT_SCHEMA: &str = "shared_hook_state_authority_v1";
const RESULT_SCHEMA: &str = "shared_hook_state_verification_result_v1";
const MAX_ARTIFACT_BYTES: usize = 64 * 1024;
const MAX_OBSERVED_BYTES: u64 = 256 * 1024;
const MAX_DRIFT_ITEMS: usize = 12;

#[derive(Clone, Copy)]
struct TargetSpec {
    name: &'static str,
    relative_path: &'static str,
    common_git_relative_path: &'static str,
}

const TARGET_SPECS: [TargetSpec; 3] = [
    TargetSpec {
        name: "pre-commit",
        relative_path: ".git/hooks/pre-commit",
        common_git_relative_path: "hooks/pre-commit",
    },
    TargetSpec {
        name: "pre-push",
        relative_path: ".git/hooks/pre-push",
        common_git_relative_path: "hooks/pre-push",
    },
    TargetSpec {
        name: "lefthook.checksum",
        relative_path: ".git/info/lefthook.checksum",
        common_git_relative_path: "info/lefthook.checksum",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthorityStatus {
    Undecided,
    Proposed,
    Decided,
}

impl AuthorityStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Undecided => "UNDECIDED",
            Self::Proposed => "PROPOSED",
            Self::Decided => "DECIDED",
        }
    }
}

#[derive(Clone)]
enum ExpectedState {
    Absent,
    Present {
        sha256: String,
        mode: String,
        artifact: Vec<u8>,
    },
}

struct Contract {
    authority: AuthorityStatus,
    decision_source: String,
    expectations: [Option<ExpectedState>; 3],
}

struct ContractError {
    code: &'static str,
}

impl ContractError {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }
}

enum ContractLoad {
    Valid(Box<Contract>),
    Missing,
    Invalid(&'static str),
}

struct ObservedState {
    state: &'static str,
    sha256: Option<String>,
    mode: Option<String>,
    size: Option<u64>,
    reason: Option<&'static str>,
    symlink: bool,
}

impl ObservedState {
    fn unavailable(reason: &'static str) -> Self {
        Self {
            state: "unavailable",
            sha256: None,
            mode: None,
            size: None,
            reason: Some(reason),
            symlink: false,
        }
    }
}

struct RepoPaths {
    root: PathBuf,
    common_git_dir: PathBuf,
}

#[derive(Clone, Copy)]
enum ResolveError {
    GitUnavailable,
    NotRepository,
    UnsafeCommonGitDir,
}

impl ResolveError {
    fn code(self) -> &'static str {
        match self {
            Self::GitUnavailable => "GIT_UNAVAILABLE",
            Self::NotRepository => "NOT_A_REPOSITORY",
            Self::UnsafeCommonGitDir => "UNSAFE_COMMON_GIT_DIR",
        }
    }
}

pub struct VerificationOutcome {
    pub result: Value,
    pub exit_code: i32,
}

/// Runs the command using only the process current directory as scope input.
///
/// No repository root, Git directory, contract path, or target path can be
/// supplied through CLI arguments.
pub fn run_cli(args: &[String]) -> i32 {
    if args.len() != 2 || args.get(1).map(String::as_str) != Some(COMMAND_NAME) {
        let outcome = input_invalid_result();
        println!("{}", render_result(&outcome.result));
        return outcome.exit_code;
    }

    let outcome = match std::env::current_dir() {
        Ok(start) => verify_at(&start),
        Err(_) => unresolved_result("CURRENT_DIRECTORY_UNAVAILABLE"),
    };
    println!("{}", render_result(&outcome.result));
    outcome.exit_code
}

fn render_result(result: &Value) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|_| {
        "{\"schema\":\"shared_hook_state_verification_result_v1\",\"status\":\"BLOCKED\",\"failClosed\":true,\"exitCode\":24}".to_string()
    })
}

fn verify_at(start: &Path) -> VerificationOutcome {
    let paths = match resolve_repo_paths(start) {
        Ok(paths) => paths,
        Err(error) => return unresolved_result(error.code()),
    };

    let contract = load_contract(&paths.root);
    let (contract_status, authority_status, decision_source, expectations, contract_error) =
        match contract {
            ContractLoad::Valid(contract) => {
                let Contract {
                    authority,
                    decision_source,
                    expectations,
                } = *contract;
                ("VALID", authority, decision_source, expectations, None)
            }
            ContractLoad::Missing => (
                "MISSING",
                AuthorityStatus::Undecided,
                "none".to_string(),
                [None, None, None],
                Some("CONTRACT_MISSING"),
            ),
            ContractLoad::Invalid(code) => (
                "INVALID",
                AuthorityStatus::Undecided,
                "none".to_string(),
                [None, None, None],
                Some(code),
            ),
        };

    let observed: [ObservedState; 3] =
        std::array::from_fn(|index| observe_target(&paths.common_git_dir, TARGET_SPECS[index]));

    let target_values: Vec<Value> = TARGET_SPECS
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            json!({
                "name": spec.name,
                "relativePath": spec.relative_path,
                "expected": expected_json(expectations[index].as_ref()),
                "observed": observed_json(&observed[index]),
            })
        })
        .collect();

    let mut drift_items = Vec::new();
    if contract_status == "VALID" && authority_status == AuthorityStatus::Decided {
        for (index, spec) in TARGET_SPECS.iter().enumerate() {
            drift_items.extend(drift_for_target(
                spec.name,
                expectations[index].as_ref(),
                &observed[index],
            ));
        }
    }
    let truncated = drift_items.len() > MAX_DRIFT_ITEMS;
    drift_items.truncate(MAX_DRIFT_ITEMS);

    let has_observation_error = observed.iter().any(|state| {
        matches!(
            state.state,
            "unavailable" | "unreadable" | "too_large" | "changed_during_read"
        )
    });

    let (status, drift_status, exit_code, fail_closed, error_code) = if contract_status != "VALID" {
        (
            "BLOCKED",
            "not_assessed",
            EXIT_CONTRACT_INVALID,
            true,
            contract_error,
        )
    } else if authority_status != AuthorityStatus::Decided {
        (
            "BLOCKED",
            "not_assessed",
            EXIT_UNDECIDED,
            true,
            Some("AUTHORITY_NOT_DECIDED"),
        )
    } else if has_observation_error {
        (
            "BLOCKED",
            "detected",
            EXIT_OBSERVATION_ERROR,
            true,
            Some("OBSERVATION_ERROR"),
        )
    } else if drift_items.is_empty() {
        ("PASS", "none", EXIT_PASS, false, None)
    } else {
        ("DRIFT", "detected", EXIT_DRIFT, true, Some("TARGET_DRIFT"))
    };

    VerificationOutcome {
        result: json!({
            "schema": RESULT_SCHEMA,
            "schemaVersion": 1,
            "status": status,
            "authorityStatus": authority_status.as_str(),
            "authority": {
                "status": authority_status.as_str(),
                "source": decision_source,
            },
            "contractStatus": contract_status,
            "contractPath": CONTRACT_RELATIVE_PATH,
            "commonGitDirStatus": "RESOLVED",
            "scope": {
                "source": "git rev-parse --git-common-dir",
                "mutation": "none",
            },
            "targets": target_values,
            "drift": {
                "status": drift_status,
                "items": drift_items,
                "truncated": truncated,
            },
            "failClosed": fail_closed,
            "errorCode": error_code,
            "exitCode": exit_code,
        }),
        exit_code,
    }
}

fn unresolved_result(error_code: &'static str) -> VerificationOutcome {
    VerificationOutcome {
        result: json!({
            "schema": RESULT_SCHEMA,
            "schemaVersion": 1,
            "status": "BLOCKED",
            "authorityStatus": "UNDECIDED",
            "authority": {
                "status": "UNDECIDED",
                "source": "none",
            },
            "contractStatus": "NOT_READ",
            "contractPath": CONTRACT_RELATIVE_PATH,
            "commonGitDirStatus": "UNRESOLVED",
            "scope": {
                "source": "git rev-parse --git-common-dir",
                "mutation": "none",
            },
            "targets": unavailable_target_values("unavailable"),
            "drift": {
                "status": "not_assessed",
                "items": [],
                "truncated": false,
            },
            "failClosed": true,
            "errorCode": error_code,
            "exitCode": EXIT_GIT_UNRESOLVED,
        }),
        exit_code: EXIT_GIT_UNRESOLVED,
    }
}

fn input_invalid_result() -> VerificationOutcome {
    VerificationOutcome {
        result: json!({
            "schema": RESULT_SCHEMA,
            "schemaVersion": 1,
            "status": "BLOCKED",
            "authorityStatus": "UNDECIDED",
            "authority": {
                "status": "UNDECIDED",
                "source": "none",
            },
            "contractStatus": "NOT_READ",
            "contractPath": CONTRACT_RELATIVE_PATH,
            "commonGitDirStatus": "UNRESOLVED",
            "scope": {
                "source": "git rev-parse --git-common-dir",
                "mutation": "none",
            },
            "targets": unavailable_target_values("input_invalid"),
            "drift": {
                "status": "not_assessed",
                "items": [],
                "truncated": false,
            },
            "failClosed": true,
            "errorCode": "INPUT_INVALID",
            "exitCode": EXIT_INPUT_INVALID,
        }),
        exit_code: EXIT_INPUT_INVALID,
    }
}

fn unavailable_target_values(reason: &'static str) -> Vec<Value> {
    TARGET_SPECS
        .iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "relativePath": spec.relative_path,
                "expected": Value::Null,
                "observed": {
                    "state": "unavailable",
                    "reason": reason,
                },
            })
        })
        .collect()
}

fn resolve_repo_paths(start: &Path) -> Result<RepoPaths, ResolveError> {
    let start = start
        .canonicalize()
        .map_err(|_| ResolveError::NotRepository)?;
    let root_text = git_value(&start, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root_text)
        .canonicalize()
        .map_err(|_| ResolveError::NotRepository)?;
    if !root.is_dir() {
        return Err(ResolveError::NotRepository);
    }

    let common_text = git_value(&start, &["rev-parse", "--git-common-dir"])?;
    let common_candidate = {
        let raw = PathBuf::from(common_text);
        if raw.is_absolute() {
            raw
        } else {
            root.join(raw)
        }
    };
    if has_symlink_component(&common_candidate) {
        return Err(ResolveError::UnsafeCommonGitDir);
    }
    let common_metadata =
        fs::symlink_metadata(&common_candidate).map_err(|_| ResolveError::UnsafeCommonGitDir)?;
    if common_metadata.file_type().is_symlink() || !common_metadata.is_dir() {
        return Err(ResolveError::UnsafeCommonGitDir);
    }
    let common_git_dir = common_candidate
        .canonicalize()
        .map_err(|_| ResolveError::UnsafeCommonGitDir)?;
    if common_git_dir.file_name().and_then(|name| name.to_str()) != Some(".git") {
        return Err(ResolveError::UnsafeCommonGitDir);
    }

    Ok(RepoPaths {
        root,
        common_git_dir,
    })
}

fn git_value(start: &Path, args: &[&str]) -> Result<String, ResolveError> {
    let output = Command::new("git")
        .current_dir(start)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .output()
        .map_err(|_| ResolveError::GitUnavailable)?;
    if !output.status.success() {
        return Err(ResolveError::NotRepository);
    }
    let raw = String::from_utf8(output.stdout).map_err(|_| ResolveError::NotRepository)?;
    let value = raw.trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(ResolveError::NotRepository);
    }
    Ok(value.to_string())
}

fn has_symlink_component(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return false,
            Err(_) => return false,
        }
    }
    false
}

fn load_contract(root: &Path) -> ContractLoad {
    let contract_path = root.join(CONTRACT_RELATIVE_PATH);
    let metadata = match fs::symlink_metadata(&contract_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return ContractLoad::Missing,
        Err(_) => return ContractLoad::Invalid("CONTRACT_UNREADABLE"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return ContractLoad::Invalid("CONTRACT_SYMLINK_OR_TYPE");
    }
    let contents = match fs::read_to_string(&contract_path) {
        Ok(contents) => contents,
        Err(_) => return ContractLoad::Invalid("CONTRACT_UNREADABLE"),
    };
    let value = match parse_strict_json(&contents) {
        Ok(value) => value,
        Err(error) => return ContractLoad::Invalid(error.code),
    };
    if validate_value_strings(&value).is_err() {
        return ContractLoad::Invalid("CONTROL_CHARACTER");
    }
    match parse_contract(&value) {
        Ok(contract) => ContractLoad::Valid(Box::new(contract)),
        Err(error) => ContractLoad::Invalid(error.code),
    }
}

fn parse_contract(value: &Value) -> Result<Contract, ContractError> {
    let root = object(value)?;
    exact_keys(root, &["schema", "schemaVersion", "authority", "targets"])?;
    if string_field(root, "schema")? != CONTRACT_SCHEMA {
        return Err(ContractError::new("SCHEMA_MISMATCH"));
    }
    if number_field(root, "schemaVersion")? != 1 {
        return Err(ContractError::new("SCHEMA_VERSION_MISMATCH"));
    }

    let authority = object(root.get("authority").expect("exact authority key"))?;
    exact_keys(
        authority,
        &[
            "status",
            "decisionId",
            "decisionSource",
            "basis",
            "mutationAllowed",
        ],
    )?;
    let authority_status = match string_field(authority, "status")? {
        "UNDECIDED" => AuthorityStatus::Undecided,
        "PROPOSED" => AuthorityStatus::Proposed,
        "DECIDED" => AuthorityStatus::Decided,
        _ => return Err(ContractError::new("UNKNOWN_AUTHORITY_STATUS")),
    };
    let decision_id = authority.get("decisionId").expect("exact decisionId key");
    match (authority_status, decision_id) {
        (AuthorityStatus::Decided, Value::String(value))
            if !value.is_empty() && value.len() <= 128 => {}
        (AuthorityStatus::Undecided | AuthorityStatus::Proposed, Value::Null) => {}
        (AuthorityStatus::Decided, _) => return Err(ContractError::new("INVALID_DECISION_ID")),
        _ => return Err(ContractError::new("NON_AUTHORITATIVE_DECISION_ID")),
    }
    let decision_source = string_field(authority, "decisionSource")?;
    if !matches!(decision_source, "human" | "repository") {
        return Err(ContractError::new("UNKNOWN_DECISION_SOURCE"));
    }
    let basis = string_field(authority, "basis")?;
    if basis.is_empty() || basis.len() > 1024 {
        return Err(ContractError::new("INVALID_BASIS"));
    }
    if authority.get("mutationAllowed").and_then(Value::as_bool) != Some(false) {
        return Err(ContractError::new("MUTATION_NOT_ALLOWED"));
    }

    let targets = root
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| ContractError::new("TARGETS_NOT_ARRAY"))?;
    if targets.len() != TARGET_SPECS.len() {
        return Err(ContractError::new("TARGET_COUNT_MISMATCH"));
    }

    let mut seen = [false; 3];
    let mut expectations: [Option<ExpectedState>; 3] = [None, None, None];
    for target_value in targets {
        let target = object(target_value)?;
        exact_keys(target, &["name", "relativePath", "expected"])?;
        let name = string_field(target, "name")?;
        let index = TARGET_SPECS
            .iter()
            .position(|spec| spec.name == name)
            .ok_or_else(|| ContractError::new("UNKNOWN_TARGET"))?;
        if seen[index] {
            return Err(ContractError::new("DUPLICATE_TARGET"));
        }
        seen[index] = true;
        if string_field(target, "relativePath")? != TARGET_SPECS[index].relative_path {
            return Err(ContractError::new("UNSAFE_TARGET_PATH"));
        }

        let expected = target.get("expected").expect("exact expected key");
        if authority_status == AuthorityStatus::Decided {
            expectations[index] = Some(parse_expected(expected)?);
        } else if !expected.is_null() {
            return Err(ContractError::new("NON_AUTHORITATIVE_EXPECTATION"));
        }
    }
    if seen.iter().any(|seen| !seen) {
        return Err(ContractError::new("TARGET_SET_INCOMPLETE"));
    }

    Ok(Contract {
        authority: authority_status,
        decision_source: decision_source.to_string(),
        expectations,
    })
}

fn parse_expected(value: &Value) -> Result<ExpectedState, ContractError> {
    let expected = object(value)?;
    let state = string_field(expected, "state")?;
    match state {
        "absent" => {
            exact_keys(expected, &["state"])?;
            Ok(ExpectedState::Absent)
        }
        "present" => {
            exact_keys(expected, &["state", "sha256", "mode", "artifact"])?;
            let sha256 = string_field(expected, "sha256")?.to_string();
            validate_sha256(&sha256)?;
            let mode = string_field(expected, "mode")?.to_string();
            parse_mode(&mode)?;
            let artifact = object(expected.get("artifact").expect("exact artifact key"))?;
            exact_keys(artifact, &["encoding", "content"])?;
            if string_field(artifact, "encoding")? != "base64" {
                return Err(ContractError::new("UNKNOWN_ARTIFACT_ENCODING"));
            }
            let content = string_field(artifact, "content")?;
            if content.len() > MAX_ARTIFACT_BYTES.div_ceil(3) * 4 {
                return Err(ContractError::new("ARTIFACT_TOO_LARGE"));
            }
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(content.as_bytes())
                .map_err(|_| ContractError::new("INVALID_ARTIFACT_BASE64"))?;
            if decoded.len() > MAX_ARTIFACT_BYTES
                || base64::engine::general_purpose::STANDARD.encode(&decoded) != content
            {
                return Err(ContractError::new("INVALID_ARTIFACT_BASE64"));
            }
            if sha256_hex(&decoded) != sha256 {
                return Err(ContractError::new("ARTIFACT_HASH_MISMATCH"));
            }
            Ok(ExpectedState::Present {
                sha256,
                mode,
                artifact: decoded,
            })
        }
        _ => Err(ContractError::new("UNKNOWN_EXPECTED_STATE")),
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, ContractError> {
    value
        .as_object()
        .ok_or_else(|| ContractError::new("EXPECTED_OBJECT"))
}

fn exact_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), ContractError> {
    if object.len() != expected.len()
        || object
            .keys()
            .any(|key| !expected.iter().any(|expected_key| *expected_key == key))
    {
        return Err(ContractError::new("UNKNOWN_OR_MISSING_FIELD"));
    }
    Ok(())
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, ContractError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ContractError::new("EXPECTED_STRING"))
}

fn number_field(object: &Map<String, Value>, field: &str) -> Result<u64, ContractError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| ContractError::new("EXPECTED_INTEGER"))
}

fn validate_sha256(value: &str) -> Result<(), ContractError> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(ContractError::new("INVALID_SHA256"));
    }
    Ok(())
}

fn parse_mode(value: &str) -> Result<u32, ContractError> {
    if value.len() != 4
        || !value.starts_with('0')
        || !value.bytes().all(|byte| (b'0'..=b'7').contains(&byte))
    {
        return Err(ContractError::new("INVALID_MODE"));
    }
    let parsed = u32::from_str_radix(value, 8).map_err(|_| ContractError::new("INVALID_MODE"))?;
    if parsed > 0o777 {
        return Err(ContractError::new("INVALID_MODE"));
    }
    Ok(parsed)
}

fn expected_json(expected: Option<&ExpectedState>) -> Value {
    match expected {
        None => Value::Null,
        Some(ExpectedState::Absent) => json!({ "state": "absent" }),
        Some(ExpectedState::Present {
            sha256,
            mode,
            artifact,
        }) => json!({
            "state": "present",
            "sha256": sha256,
            "mode": mode,
            "artifact": {
                "encoding": "base64",
                "size": artifact.len(),
                "sha256": sha256,
            },
        }),
    }
}

fn observed_json(observed: &ObservedState) -> Value {
    let mut result = Map::new();
    result.insert("state".to_string(), json!(observed.state));
    if let Some(sha256) = &observed.sha256 {
        result.insert("sha256".to_string(), json!(sha256));
    }
    if let Some(mode) = &observed.mode {
        result.insert("mode".to_string(), json!(mode));
    }
    if let Some(size) = observed.size {
        result.insert("size".to_string(), json!(size));
    }
    if let Some(reason) = observed.reason {
        result.insert("reason".to_string(), json!(reason));
    }
    if observed.symlink {
        result.insert("symlink".to_string(), json!(true));
    }
    Value::Object(result)
}

fn observe_target(common_git_dir: &Path, spec: TargetSpec) -> ObservedState {
    let target_path = common_git_dir.join(spec.common_git_relative_path);
    let metadata = match fs::symlink_metadata(&target_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return ObservedState {
                state: "absent",
                sha256: None,
                mode: None,
                size: None,
                reason: None,
                symlink: false,
            };
        }
        Err(_) => return ObservedState::unavailable("LSTAT_FAILED"),
    };
    if metadata.file_type().is_symlink() {
        return ObservedState {
            state: "symlink",
            sha256: None,
            mode: None,
            size: Some(metadata.len()),
            reason: Some("SYMLINK_REJECTED"),
            symlink: true,
        };
    }
    if !metadata.is_file() {
        return ObservedState {
            state: "other",
            sha256: None,
            mode: None,
            size: Some(metadata.len()),
            reason: Some("NOT_REGULAR_FILE"),
            symlink: false,
        };
    }
    let mode = mode_string(&metadata);
    let size = metadata.len();
    if size > MAX_OBSERVED_BYTES {
        return ObservedState {
            state: "too_large",
            sha256: None,
            mode: Some(mode),
            size: Some(size),
            reason: Some("OBSERVED_SIZE_LIMIT"),
            symlink: false,
        };
    }
    let bytes = match fs::read(&target_path) {
        Ok(bytes) => bytes,
        Err(_) => return ObservedState::unavailable("READ_FAILED"),
    };
    let after = match fs::symlink_metadata(&target_path) {
        Ok(after) => after,
        Err(_) => return ObservedState::unavailable("POST_READ_LSTAT_FAILED"),
    };
    if after.file_type().is_symlink()
        || !after.is_file()
        || after.len() != size
        || after.len() != bytes.len() as u64
        || mode_string(&after) != mode
    {
        return ObservedState {
            state: "changed_during_read",
            sha256: None,
            mode: Some(mode),
            size: Some(after.len()),
            reason: Some("TARGET_CHANGED_DURING_READ"),
            symlink: after.file_type().is_symlink(),
        };
    }
    ObservedState {
        state: "present",
        sha256: Some(sha256_hex(&bytes)),
        mode: Some(mode),
        size: Some(bytes.len() as u64),
        reason: None,
        symlink: false,
    }
}

#[cfg(unix)]
fn mode_string(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    format!("{:04o}", metadata.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_string(_metadata: &fs::Metadata) -> String {
    "0000".to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn drift_for_target(
    name: &str,
    expected: Option<&ExpectedState>,
    observed: &ObservedState,
) -> Vec<Value> {
    let Some(expected) = expected else {
        return Vec::new();
    };
    match expected {
        ExpectedState::Absent => match observed.state {
            "absent" => Vec::new(),
            "symlink" => vec![json!({ "name": name, "kind": "symlink" })],
            "present" => vec![json!({ "name": name, "kind": "unexpected_present" })],
            _ => vec![json!({ "name": name, "kind": "wrong_type" })],
        },
        ExpectedState::Present { sha256, mode, .. } => {
            if observed.state != "present" {
                let kind = match observed.state {
                    "absent" => "missing",
                    "symlink" => "symlink",
                    "unavailable" | "unreadable" | "too_large" | "changed_during_read" => {
                        "observation_unavailable"
                    }
                    _ => "wrong_type",
                };
                return vec![json!({ "name": name, "kind": kind })];
            }
            let mut drift = Vec::new();
            if observed.sha256.as_deref() != Some(sha256.as_str()) {
                drift.push(json!({ "name": name, "kind": "hash_mismatch" }));
            }
            if observed.mode.as_deref() != Some(mode.as_str()) {
                drift.push(json!({ "name": name, "kind": "mode_mismatch" }));
            }
            drift
        }
    }
}

fn validate_value_strings(value: &Value) -> Result<(), ContractError> {
    match value {
        Value::String(value) if value.chars().any(is_unsafe_control) => {
            Err(ContractError::new("CONTROL_CHARACTER"))
        }
        Value::Array(values) => values.iter().try_for_each(validate_value_strings),
        Value::Object(values) => {
            if values.keys().any(|key| key.chars().any(is_unsafe_control)) {
                return Err(ContractError::new("CONTROL_CHARACTER"));
            }
            values.values().try_for_each(validate_value_strings)
        }
        _ => Ok(()),
    }
}

fn is_unsafe_control(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{200b}'
                | '\u{200c}'
                | '\u{200d}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202e}'
                | '\u{2060}'
                | '\u{2066}'
                | '\u{2067}'
                | '\u{2069}'
        )
}

fn parse_strict_json(contents: &str) -> Result<Value, ContractError> {
    let mut deserializer = serde_json::Deserializer::from_str(contents);
    let value = StrictValue::deserialize(&mut deserializer)
        .map_err(|_| ContractError::new("INVALID_JSON"))?
        .0;
    deserializer
        .end()
        .map_err(|_| ContractError::new("TRAILING_JSON"))?;
    Ok(value)
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrictVisitor;

        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = Value;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON value with unique object keys")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Number(value.into()))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Number(value.into()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                serde_json::Number::from_f64(value)
                    .map(Value::Number)
                    .ok_or_else(|| E::custom("invalid JSON number"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::String(value))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Null)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Value::Null)
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<StrictValue>()? {
                    values.push(value.0);
                }
                Ok(Value::Array(values))
            }

            fn visit_map<A>(self, mut map_access: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = Map::new();
                while let Some(key) = map_access.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON object key"));
                    }
                    let value = map_access.next_value::<StrictValue>()?;
                    values.insert(key, value.0);
                }
                Ok(Value::Object(values))
            }
        }

        deserializer.deserialize_any(StrictVisitor).map(StrictValue)
    }
}

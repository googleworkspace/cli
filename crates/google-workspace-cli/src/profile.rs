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

//! Named auth profiles.
//!
//! A profile selects which set of per-account files (credentials, token
//! caches, timezone cache) the CLI uses. The `default` profile maps to the
//! config directory itself; any other profile maps to
//! `<config_dir>/profiles/<name>/`.
//!
//! Selection precedence: `--profile NAME` global flag >
//! `GOOGLE_WORKSPACE_CLI_PROFILE` (empty = unset) > `default`.
//! Invalid names are always rejected — never silently replaced by `default`.
//!
//! Everything except the process-wide [`set_active`] / [`active`] pair is a
//! pure function so it can be unit-tested without touching global state.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::error::GwsError;

pub const DEFAULT_PROFILE: &str = "default";
pub const PROFILE_ENV_VAR: &str = "GOOGLE_WORKSPACE_CLI_PROFILE";
const PROFILE_FLAG: &str = "--profile";
const PROFILES_SUBDIR: &str = "profiles";
const MAX_PROFILE_NAME_LEN: usize = 64;
const TOKEN_ENV_VAR: &str = "GOOGLE_WORKSPACE_CLI_TOKEN";
const CREDENTIALS_FILE_ENV_VAR: &str = "GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE";

/// Windows reserved device names; a directory with one of these names cannot
/// be created there, so reject them everywhere for portable configs.
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Where the active profile name came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    Default,
    Env,
    Flag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProfile {
    pub name: String,
    pub source: ProfileSource,
}

impl ActiveProfile {
    pub fn default_profile() -> Self {
        Self {
            name: DEFAULT_PROFILE.to_string(),
            source: ProfileSource::Default,
        }
    }

    pub fn is_default(&self) -> bool {
        self.name == DEFAULT_PROFILE
    }
}

/// Validates a profile name against `^[a-z0-9_][a-z0-9_-]{0,63}$` and rejects
/// Windows device names. Lowercase-only so that case-insensitive filesystems
/// cannot alias two distinct profiles to the same directory.
pub fn validate_profile_name(name: &str) -> Result<(), GwsError> {
    let mut chars = name.chars();
    let valid_first = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    let valid_rest =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !valid_first || !valid_rest || name.len() > MAX_PROFILE_NAME_LEN {
        return Err(GwsError::Validation(format!(
            "Invalid profile name {name:?}: must be 1-{MAX_PROFILE_NAME_LEN} characters of \
             lowercase letters, digits, '_' or '-', and must not start with '-'"
        )));
    }
    if RESERVED_NAMES.contains(&name) {
        return Err(GwsError::Validation(format!(
            "Invalid profile name {name:?}: reserved device name"
        )));
    }
    Ok(())
}

/// Resolves the active profile from the `--profile` flag value and the
/// `GOOGLE_WORKSPACE_CLI_PROFILE` env var value (empty string = unset).
pub fn resolve_profile(flag: Option<&str>, env: Option<&str>) -> Result<ActiveProfile, GwsError> {
    if let Some(name) = flag {
        validate_profile_name(name)?;
        return Ok(ActiveProfile {
            name: name.to_string(),
            source: ProfileSource::Flag,
        });
    }
    match env {
        Some(name) if !name.is_empty() => {
            validate_profile_name(name).map_err(|e| {
                GwsError::Validation(format!("{PROFILE_ENV_VAR}: {}", error_message(e)))
            })?;
            Ok(ActiveProfile {
                name: name.to_string(),
                source: ProfileSource::Env,
            })
        }
        _ => Ok(ActiveProfile::default_profile()),
    }
}

fn error_message(err: GwsError) -> String {
    match err {
        GwsError::Validation(msg) => msg,
        other => other.to_string(),
    }
}

/// Returns the directory holding a profile's per-account files.
pub fn profile_dir_for(base: &Path, name: &str) -> PathBuf {
    if name == DEFAULT_PROFILE {
        base.to_path_buf()
    } else {
        base.join(PROFILES_SUBDIR).join(name)
    }
}

/// Extracts the global `--profile NAME` / `--profile=NAME` flag from argv and
/// returns the argv without it.
///
/// `args[0]` (the program name) is kept as-is. Only whole-argument matches are
/// recognised, and everything after a literal `--` is passed through verbatim.
/// A missing or empty value, or repeated conflicting values, are rejected.
pub fn extract_and_strip_profile(
    args: &[String],
) -> Result<(Option<String>, Vec<String>), GwsError> {
    let mut profile: Option<String> = None;
    let mut out = Vec::with_capacity(args.len());
    let mut iter = args.iter();
    if let Some(program) = iter.next() {
        out.push(program.clone());
    }

    while let Some(arg) = iter.next() {
        if arg == "--" {
            out.push(arg.clone());
            out.extend(iter.cloned());
            break;
        }
        let value = if arg == PROFILE_FLAG {
            iter.next().cloned().ok_or_else(|| {
                GwsError::Validation(format!("{PROFILE_FLAG} requires a profile name"))
            })?
        } else if let Some(v) = arg.strip_prefix("--profile=") {
            v.to_string()
        } else {
            out.push(arg.clone());
            continue;
        };

        if value.is_empty() {
            return Err(GwsError::Validation(format!(
                "{PROFILE_FLAG} requires a non-empty profile name"
            )));
        }
        match &profile {
            Some(existing) if *existing != value => {
                return Err(GwsError::Validation(format!(
                    "Conflicting {PROFILE_FLAG} values: {existing:?} and {value:?}"
                )));
            }
            _ => profile = Some(value),
        }
    }

    Ok((profile, out))
}

/// Checks the selected profile against credential env vars that bypass
/// profile storage (`GOOGLE_WORKSPACE_CLI_TOKEN`,
/// `GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE`).
///
/// An explicit `--profile NAME` combined with one of them is ambiguous and is
/// rejected. A profile selected via the env var only gets a warning (returned
/// as `Some(message)`), since the env-var credentials keep their documented
/// highest priority.
pub fn check_credential_env_conflict(
    profile: &ActiveProfile,
    token_set: bool,
    credentials_file_set: bool,
) -> Result<Option<String>, GwsError> {
    if profile.is_default() {
        return Ok(None);
    }
    let overriding: Vec<&str> = [
        (token_set, TOKEN_ENV_VAR),
        (credentials_file_set, CREDENTIALS_FILE_ENV_VAR),
    ]
    .into_iter()
    .filter_map(|(set, var)| set.then_some(var))
    .collect();
    if overriding.is_empty() {
        return Ok(None);
    }
    let vars = overriding.join(" and ");
    match profile.source {
        ProfileSource::Flag => Err(GwsError::Validation(format!(
            "{PROFILE_FLAG} {} cannot be combined with {vars}, which takes precedence over \
             profile credentials. Unset {vars} or drop {PROFILE_FLAG}.",
            profile.name
        ))),
        _ => Ok(Some(format!(
            "warning: {vars} is set and takes precedence over the credentials of profile '{}'",
            profile.name
        ))),
    }
}

/// Resolves the active profile from argv and the given env values.
///
/// Returns the profile, the argv with `--profile` stripped, and an optional
/// warning to print to stderr.
pub fn select(
    args: &[String],
    env_profile: Option<&str>,
    token_set: bool,
    credentials_file_set: bool,
) -> Result<(ActiveProfile, Vec<String>, Option<String>), GwsError> {
    let (flag, stripped) = extract_and_strip_profile(args)?;
    let profile = resolve_profile(flag.as_deref(), env_profile)?;
    let warning = check_credential_env_conflict(&profile, token_set, credentials_file_set)?;
    Ok((profile, stripped, warning))
}

static ACTIVE_PROFILE: OnceLock<ActiveProfile> = OnceLock::new();

/// Resolves the active profile from the process argv and environment, stores
/// it for the rest of the process, and returns the argv without `--profile`.
pub fn init(args: &[String]) -> Result<Vec<String>, GwsError> {
    let env_profile = match std::env::var(PROFILE_ENV_VAR) {
        Ok(v) => Some(v),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(GwsError::Validation(format!(
                "{PROFILE_ENV_VAR} is not valid UTF-8"
            )))
        }
    };
    let is_set = |var: &str| std::env::var_os(var).is_some_and(|v| !v.is_empty());
    let (profile, stripped, warning) = select(
        args,
        env_profile.as_deref(),
        is_set(TOKEN_ENV_VAR),
        is_set(CREDENTIALS_FILE_ENV_VAR),
    )?;
    if let Some(warning) = warning {
        eprintln!("{warning}");
    }
    set_active(profile);
    Ok(stripped)
}

/// Stores the active profile. Only the first call has an effect.
pub fn set_active(profile: ActiveProfile) {
    let _ = ACTIVE_PROFILE.set(profile);
}

/// Returns the active profile (`default` if none was set).
pub fn active() -> ActiveProfile {
    ACTIVE_PROFILE
        .get()
        .cloned()
        .unwrap_or_else(ActiveProfile::default_profile)
}

/// Directory holding the active profile's per-account files.
pub fn profile_dir() -> PathBuf {
    profile_dir_for(&crate::auth_commands::config_dir(), &active().name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn named(name: &str, source: ProfileSource) -> ActiveProfile {
        ActiveProfile {
            name: name.to_string(),
            source,
        }
    }

    #[test]
    fn validate_accepts_valid_names() {
        for name in [
            "default",
            "work",
            "a",
            "_x",
            "0",
            "my-work_2",
            &"a".repeat(64),
        ] {
            assert!(
                validate_profile_name(name).is_ok(),
                "{name} should be valid"
            );
        }
    }

    #[test]
    fn validate_rejects_invalid_names() {
        let too_long = "a".repeat(65);
        for name in [
            "",
            "../x",
            "..",
            "a/b",
            r"a\b",
            "Work",
            "WORK",
            "work.profile",
            "--help",
            "-x",
            "a b",
            "a\n",
            "é",
            too_long.as_str(),
        ] {
            assert!(
                validate_profile_name(name).is_err(),
                "{name:?} should be rejected"
            );
        }
    }

    #[test]
    fn validate_rejects_windows_device_names() {
        for name in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
            let err = validate_profile_name(name).unwrap_err();
            assert!(err.to_string().contains("reserved"), "{name}: {err}");
        }
        assert!(validate_profile_name("com10").is_ok());
        assert!(validate_profile_name("console").is_ok());
    }

    #[test]
    fn resolve_precedence_flag_over_env_over_default() {
        assert_eq!(
            resolve_profile(Some("work"), Some("home")).unwrap(),
            named("work", ProfileSource::Flag)
        );
        assert_eq!(
            resolve_profile(None, Some("home")).unwrap(),
            named("home", ProfileSource::Env)
        );
        assert_eq!(
            resolve_profile(None, None).unwrap(),
            ActiveProfile::default_profile()
        );
    }

    #[test]
    fn resolve_treats_empty_env_as_unset() {
        assert_eq!(
            resolve_profile(None, Some("")).unwrap(),
            ActiveProfile::default_profile()
        );
    }

    #[test]
    fn resolve_rejects_invalid_env_instead_of_falling_back() {
        let err = resolve_profile(None, Some("../x")).unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)));
        assert!(err.to_string().contains(PROFILE_ENV_VAR));
        assert!(resolve_profile(None, Some("Bad")).is_err());
    }

    #[test]
    fn resolve_rejects_invalid_flag_even_with_valid_env() {
        assert!(resolve_profile(Some("Bad"), Some("work")).is_err());
        assert!(resolve_profile(Some(""), None).is_err());
    }

    #[test]
    fn resolve_explicit_default_flag() {
        let p = resolve_profile(Some("default"), Some("work")).unwrap();
        assert!(p.is_default());
        assert_eq!(p.source, ProfileSource::Flag);
    }

    #[test]
    fn profile_dir_for_default_is_base() {
        let base = Path::new("/cfg/gws");
        assert_eq!(profile_dir_for(base, DEFAULT_PROFILE), base);
    }

    #[test]
    fn profile_dir_for_named_is_under_profiles() {
        let base = Path::new("/cfg/gws");
        assert_eq!(
            profile_dir_for(base, "work"),
            base.join("profiles").join("work")
        );
    }

    #[test]
    fn strip_leading_flag() {
        let (p, rest) =
            extract_and_strip_profile(&argv(&["gws", "--profile", "work", "auth", "status"]))
                .unwrap();
        assert_eq!(p.as_deref(), Some("work"));
        assert_eq!(rest, argv(&["gws", "auth", "status"]));
    }

    #[test]
    fn strip_middle_and_trailing_flag() {
        let (p, rest) = extract_and_strip_profile(&argv(&[
            "gws",
            "drive",
            "files",
            "--profile",
            "work",
            "list",
        ]))
        .unwrap();
        assert_eq!(p.as_deref(), Some("work"));
        assert_eq!(rest, argv(&["gws", "drive", "files", "list"]));

        let (p, rest) =
            extract_and_strip_profile(&argv(&["gws", "auth", "status", "--profile=home"])).unwrap();
        assert_eq!(p.as_deref(), Some("home"));
        assert_eq!(rest, argv(&["gws", "auth", "status"]));
    }

    #[test]
    fn strip_equals_form() {
        let (p, rest) =
            extract_and_strip_profile(&argv(&["gws", "--profile=work", "auth"])).unwrap();
        assert_eq!(p.as_deref(), Some("work"));
        assert_eq!(rest, argv(&["gws", "auth"]));
    }

    #[test]
    fn strip_without_flag_is_identity() {
        let args = argv(&["gws", "drive", "files", "list", "--format", "table"]);
        let (p, rest) = extract_and_strip_profile(&args).unwrap();
        assert_eq!(p, None);
        assert_eq!(rest, args);
        assert_eq!(extract_and_strip_profile(&[]).unwrap(), (None, vec![]));
    }

    #[test]
    fn strip_rejects_missing_value() {
        let err = extract_and_strip_profile(&argv(&["gws", "auth", "--profile"])).unwrap_err();
        assert!(err.to_string().contains("requires a profile name"));
    }

    #[test]
    fn strip_rejects_empty_value() {
        assert!(extract_and_strip_profile(&argv(&["gws", "--profile", "", "auth"])).is_err());
        assert!(extract_and_strip_profile(&argv(&["gws", "--profile=", "auth"])).is_err());
    }

    #[test]
    fn strip_rejects_conflicting_repeats() {
        let err = extract_and_strip_profile(&argv(&[
            "gws",
            "--profile",
            "work",
            "auth",
            "--profile=home",
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("Conflicting"));
    }

    #[test]
    fn strip_allows_identical_repeats() {
        let (p, rest) = extract_and_strip_profile(&argv(&[
            "gws",
            "--profile",
            "work",
            "auth",
            "--profile=work",
        ]))
        .unwrap();
        assert_eq!(p.as_deref(), Some("work"));
        assert_eq!(rest, argv(&["gws", "auth"]));
    }

    #[test]
    fn strip_leaves_args_after_double_dash_intact() {
        let (p, rest) = extract_and_strip_profile(&argv(&[
            "gws",
            "drive",
            "--",
            "--profile",
            "work",
            "--profile=x",
        ]))
        .unwrap();
        assert_eq!(p, None);
        assert_eq!(
            rest,
            argv(&["gws", "drive", "--", "--profile", "work", "--profile=x"])
        );
    }

    #[test]
    fn strip_only_matches_whole_args() {
        let args = argv(&["gws", "drive", "--profiles", "x", "--profile-x=y"]);
        let (p, rest) = extract_and_strip_profile(&args).unwrap();
        assert_eq!(p, None);
        assert_eq!(rest, args);
    }

    #[test]
    fn strip_profile_named_like_a_service() {
        let (p, rest) = extract_and_strip_profile(&argv(&[
            "gws",
            "--profile",
            "drive",
            "drive",
            "files",
            "list",
        ]))
        .unwrap();
        assert_eq!(p.as_deref(), Some("drive"));
        assert_eq!(rest, argv(&["gws", "drive", "files", "list"]));
        assert_eq!(
            crate::filter_args_for_subcommand(&rest, "drive"),
            argv(&["gws", "files", "list"])
        );
    }

    #[test]
    fn strip_flag_value_that_looks_like_a_flag_is_rejected_by_validation() {
        let (p, _) = extract_and_strip_profile(&argv(&["gws", "--profile", "--help"])).unwrap();
        assert!(resolve_profile(p.as_deref(), None).is_err());
    }

    #[test]
    fn conflict_none_for_default_profile() {
        let p = resolve_profile(Some("default"), None).unwrap();
        assert_eq!(check_credential_env_conflict(&p, true, true).unwrap(), None);
    }

    #[test]
    fn conflict_none_without_credential_env() {
        let p = named("work", ProfileSource::Flag);
        assert_eq!(
            check_credential_env_conflict(&p, false, false).unwrap(),
            None
        );
    }

    #[test]
    fn conflict_errors_for_flag_selected_profile() {
        let p = named("work", ProfileSource::Flag);
        let err = check_credential_env_conflict(&p, true, false).unwrap_err();
        assert!(matches!(err, GwsError::Validation(_)));
        assert!(err.to_string().contains(TOKEN_ENV_VAR));

        let err = check_credential_env_conflict(&p, false, true).unwrap_err();
        assert!(err.to_string().contains(CREDENTIALS_FILE_ENV_VAR));
    }

    #[test]
    fn conflict_warns_for_env_selected_profile() {
        let p = named("work", ProfileSource::Env);
        let warning = check_credential_env_conflict(&p, true, true)
            .unwrap()
            .unwrap();
        assert!(warning.starts_with("warning:"));
        assert!(warning.contains(TOKEN_ENV_VAR));
        assert!(warning.contains(CREDENTIALS_FILE_ENV_VAR));
        assert!(!warning.contains('\n'));
    }

    #[test]
    fn select_combines_strip_resolve_and_conflict() {
        let (p, rest, warning) = select(
            &argv(&["gws", "--profile", "work", "auth", "status"]),
            Some("home"),
            false,
            false,
        )
        .unwrap();
        assert_eq!(p, named("work", ProfileSource::Flag));
        assert_eq!(rest, argv(&["gws", "auth", "status"]));
        assert_eq!(warning, None);

        let (p, _, warning) = select(&argv(&["gws", "auth"]), Some("home"), true, false).unwrap();
        assert_eq!(p, named("home", ProfileSource::Env));
        assert!(warning.is_some());

        assert!(select(&argv(&["gws", "--profile", "work"]), None, true, false).is_err());
        assert!(select(&argv(&["gws", "auth"]), Some("../x"), false, false).is_err());
    }

    #[test]
    #[serial_test::serial]
    fn active_defaults_when_unset() {
        // The global is never set in unit tests.
        assert!(active().is_default());
        assert_eq!(profile_dir(), crate::auth_commands::config_dir());
    }
}

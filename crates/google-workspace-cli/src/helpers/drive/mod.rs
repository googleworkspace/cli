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

pub(crate) mod api;
pub(crate) mod path;
pub(crate) mod sync;

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::executor;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

pub struct DriveHelper;

impl Helper for DriveHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+upload")
                .about("[Helper] Upload a file with automatic metadata")
                .arg(
                    Arg::new("file")
                        .help("Path to file to upload")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("parent")
                        .long("parent")
                        .help("Parent folder ID")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("name")
                        .long("name")
                        .help("Target filename (defaults to source filename)")
                        .value_name("NAME"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws drive +upload ./report.pdf
  gws drive +upload ./report.pdf --parent FOLDER_ID
  gws drive +upload ./data.csv --name 'Sales Data.csv'

TIPS:
  MIME type is detected automatically.
  Filename is inferred from the local path unless --name is given.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+path")
                .about("[Helper] Resolve a My Drive or shared-drive path to a file id")
                .arg(
                    Arg::new("path")
                        .help("Path such as 'Company/design/social' or 'Shared drives/<drive>/a/b'")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("create")
                        .long("create")
                        .help("Create missing folders along the path")
                        .action(ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws drive +path 'Company/design/social'
  gws drive +path 'Shared drives/Engineering/specs/2026'
  gws drive +path 'Reports/Q1\\/Q2' --create
  gws drive +path 'Archive/2026/posters' --create --dry-run

TIPS:
  The path is resolved one folder name at a time. If two items in a folder
  share a name, the command lists their ids and stops; it never picks one.
  Paths start at My Drive ('My Drive/' is optional); 'Shared drives/<name>/'
  starts at a shared drive. Write a slash inside a name as '\\/'.
  With --dry-run, --create reports the folders it would make.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+sync")
                .about("[Helper] Make a Drive folder match a local directory")
                .arg(
                    Arg::new("local")
                        .help("Local directory (relative to the current directory)")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("folder")
                        .help("Drive folder: id, URL or path (created if missing)")
                        .required(true)
                        .index(2),
                )
                .arg(
                    Arg::new("delete")
                        .long("delete")
                        .help("Move to the trash remote items that are not in the local directory")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("ids")
                        .long("ids")
                        .help("Write a JSON map of relative path -> Drive id to this file")
                        .value_name("FILE"),
                )
                .arg(
                    Arg::new("exclude")
                        .long("exclude")
                        .help("Skip local names (or relative paths, if the pattern has a '/') matching this glob; repeatable")
                        .value_name("GLOB")
                        .action(ArgAction::Append),
                )
                .after_help(
                    "\
EXAMPLES:
  gws drive +sync ./out 'Company/design/social/launch'
  gws drive +sync ./out FOLDER_ID --dry-run
  gws drive +sync ./posters 'Posters' --delete --ids drive.json
  gws drive +sync ./site 'Site' --exclude '*.tmp' --exclude 'drafts/*'

TIPS:
  New files are uploaded. Changed files (by size and MD5) are updated in
  place, so their ids and share links stay the same. Unchanged files are
  not touched. Subdirectories are synced recursively.
  --delete moves extra remote items to the trash (never deletes them
  permanently) and leaves Google Docs, Sheets and Slides alone.
  The whole plan is checked before anything changes: if two remote items
  share a name, or a file sits where a folder should be, the command lists
  every such path and changes nothing.
  .DS_Store, Thumbs.db, desktop.ini and .git are always skipped; symlinks
  are not followed.",
                ),
        );
        cmd
    }

    fn handle<'a>(
        &'a self,
        doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(matches) = matches.subcommand_matches("+path") {
                handle_path(matches).await?;
                return Ok(true);
            }
            if let Some(matches) = matches.subcommand_matches("+sync") {
                handle_sync(matches).await?;
                return Ok(true);
            }
            if let Some(matches) = matches.subcommand_matches("+upload") {
                let file_path = matches.get_one::<String>("file").unwrap();
                let parent_id = matches.get_one::<String>("parent");
                let name_arg = matches.get_one::<String>("name");

                // Determine filename
                let filename = determine_filename(file_path, name_arg.map(|s| s.as_str()))?;

                // Find method: files.create
                let files_res = doc
                    .resources
                    .get("files")
                    .ok_or_else(|| GwsError::Discovery("Resource 'files' not found".to_string()))?;
                let create_method = files_res.methods.get("create").ok_or_else(|| {
                    GwsError::Discovery("Method 'files.create' not found".to_string())
                })?;

                // Build metadata
                let metadata = build_metadata(&filename, parent_id.map(|s| s.as_str()));

                let body_str = metadata.to_string();

                let scopes: Vec<&str> = create_method.scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scopes).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) if matches.get_flag("dry-run") => (None, executor::AuthMethod::None),
                    Err(e) => return Err(GwsError::Auth(format!("Drive auth failed: {e}"))),
                };

                executor::execute_method(
                    doc,
                    create_method,
                    None,
                    Some(&body_str),
                    token.as_deref(),
                    auth_method,
                    None,
                    Some(executor::UploadSource::File {
                        path: file_path,
                        content_type: None,
                    }),
                    matches.get_flag("dry-run"),
                    &executor::PaginationConfig::default(),
                    None,
                    &crate::helpers::modelarmor::SanitizeMode::Warn,
                    &crate::formatter::OutputFormat::default(),
                    false,
                )
                .await?;

                return Ok(true);
            }
            Ok(false)
        })
    }
}

/// Prints a helper's JSON result in the format chosen with `--format`.
pub(crate) fn print_result(matches: &ArgMatches, value: &Value) {
    let format = matches
        .get_one::<String>("format")
        .map(|s| crate::formatter::OutputFormat::from_str(s))
        .unwrap_or_default();
    println!("{}", crate::formatter::format_value(value, &format));
}

async fn handle_path(matches: &ArgMatches) -> Result<(), GwsError> {
    let input = matches.get_one::<String>("path").unwrap();
    let parsed = path::parse_drive_path(input)?;
    let dry_run = matches.get_flag("dry-run");
    let create = match (matches.get_flag("create"), dry_run) {
        (false, _) => path::Create::No,
        (true, false) => path::Create::Yes,
        (true, true) => path::Create::DryRun,
    };
    let client = api::DriveClient::connect(&[api::DRIVE_SCOPE]).await?;
    let resolved = path::resolve_path(&client, &parsed, path::Expect::Any, create).await?;
    for p in &resolved.created {
        eprintln!("created folder {}", crate::output::sanitize_for_terminal(p));
    }
    let value = serde_json::to_value(&resolved)
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to encode result: {e}")))?;
    print_result(matches, &value);
    Ok(())
}

async fn handle_sync(matches: &ArgMatches) -> Result<(), GwsError> {
    let local_arg = matches.get_one::<String>("local").unwrap();
    let local = crate::validate::validate_safe_dir_path(local_arg)?;
    if !local.is_dir() {
        return Err(GwsError::Validation(format!(
            "'{local_arg}' is not a directory"
        )));
    }
    let target = matches.get_one::<String>("folder").unwrap();
    let ids_path = match matches.get_one::<String>("ids") {
        Some(p) => Some(crate::validate::validate_safe_file_path(p, "--ids")?),
        None => None,
    };
    let mut excludes: Vec<String> = sync::DEFAULT_EXCLUDES
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(user) = matches.get_many::<String>("exclude") {
        for pattern in user {
            crate::validate::reject_dangerous_chars(pattern, "--exclude")?;
            excludes.push(pattern.clone());
        }
    }
    let dry_run = matches.get_flag("dry-run");
    let client = api::DriveClient::connect(&[api::DRIVE_SCOPE]).await?;
    let summary = sync::run_sync(
        &client,
        sync::SyncOptions {
            local: &local,
            target,
            dry_run,
            delete: matches.get_flag("delete"),
            excludes,
            ids_path: if dry_run { None } else { ids_path },
        },
    )
    .await?;
    eprintln!("{}", summary.line());
    let value = serde_json::to_value(&summary)
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to encode summary: {e}")))?;
    print_result(matches, &value);
    if !summary.errors.is_empty() {
        return Err(GwsError::Other(anyhow::anyhow!(
            "{} item(s) failed to sync; see \"errors\" above",
            summary.errors.len()
        )));
    }
    Ok(())
}

fn determine_filename(file_path: &str, name_arg: Option<&str>) -> Result<String, GwsError> {
    if let Some(n) = name_arg {
        Ok(n.to_string())
    } else {
        Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| GwsError::Validation("Invalid file path".to_string()))
    }
}

fn build_metadata(filename: &str, parent_id: Option<&str>) -> Value {
    let mut metadata = json!({
        "name": filename
    });

    if let Some(parent) = parent_id {
        metadata["parents"] = json!([parent]);
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_filename_explicit() {
        assert_eq!(
            determine_filename("path/to/file.txt", Some("custom.txt")).unwrap(),
            "custom.txt"
        );
    }

    #[test]
    fn test_determine_filename_from_path() {
        assert_eq!(
            determine_filename("path/to/file.txt", None).unwrap(),
            "file.txt"
        );
    }

    #[test]
    fn test_determine_filename_invalid_path() {
        assert!(determine_filename("", None).is_err());
        assert!(determine_filename("/", None).is_err()); // Root has no filename component usually
    }

    #[test]
    fn test_build_metadata_no_parent() {
        let meta = build_metadata("file.txt", None);
        assert_eq!(meta["name"], "file.txt");
        assert!(meta.get("parents").is_none());
    }

    #[test]
    fn test_build_metadata_with_parent() {
        let meta = build_metadata("file.txt", Some("folder123"));
        assert_eq!(meta["name"], "file.txt");
        assert_eq!(meta["parents"][0], "folder123");
    }
}

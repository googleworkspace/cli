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

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::executor;
use anyhow::Context;
use clap::{Arg, ArgMatches, Command};
use serde_json::json;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

pub struct ScriptHelper;

impl Helper for ScriptHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+push")
                .about("[Helper] Upload local files to an Apps Script project")
                .arg(
                    Arg::new("script")
                        .long("script")
                        .help("Script Project ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("dir")
                        .long("dir")
                        .help("Directory containing script files (defaults to current dir)")
                        .value_name("DIR"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws script +push --script SCRIPT_ID
  gws script +push --script SCRIPT_ID --dir ./src

TIPS:
  Supports .gs, .js, .html, and appsscript.json files.
  Skips hidden files and node_modules automatically.
  This replaces ALL files in the project.",
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
            if let Some(matches) = matches.subcommand_matches("+push") {
                let script_id = matches.get_one::<String>("script").unwrap();
                let dir_path = matches
                    .get_one::<String>("dir")
                    .map(|s| s.as_str())
                    .unwrap_or(".");
                let safe_dir = crate::validate::validate_safe_dir_path(dir_path)?;

                let mut files = Vec::new();
                visit_dirs(&safe_dir, &mut files)?;

                if files.is_empty() {
                    return Err(GwsError::Validation(format!(
                        "No eligible files found in '{}'",
                        dir_path
                    )));
                }

                // Find method: projects.updateContent
                let projects_res = doc.resources.get("projects").ok_or_else(|| {
                    GwsError::Discovery("Resource 'projects' not found".to_string())
                })?;
                let update_method = projects_res.methods.get("updateContent").ok_or_else(|| {
                    GwsError::Discovery("Method 'projects.updateContent' not found".to_string())
                })?;

                // Build body
                let body = json!({
                    "files": files
                });
                let body_str = body.to_string();

                let scopes: Vec<&str> = update_method.scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scopes).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) => (None, executor::AuthMethod::None),
                };

                let params = json!({
                    "scriptId": script_id
                });
                let params_str = params.to_string();

                executor::execute_method(
                    doc,
                    update_method,
                    Some(&params_str),
                    Some(&body_str),
                    token.as_deref(),
                    auth_method,
                    None,
                    None,
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

fn visit_dirs(dir: &Path, files: &mut Vec<serde_json::Value>) -> Result<(), GwsError> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).context("Failed to read dir")? {
            let entry = entry.context("Failed to read entry")?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, files)?;
            } else if let Some(file_obj) = process_file(&path)? {
                files.push(file_obj);
            }
        }
    }
    Ok(())
}

fn process_file(path: &Path) -> Result<Option<serde_json::Value>, GwsError> {
    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    // Skip hidden files, node_modules, .git, etc. (basic filtering)
    if filename.starts_with('.') || path.components().any(|c| c.as_os_str() == "node_modules") {
        return Ok(None);
    }

    let (type_val, name_val) = match extension {
        "gs" | "js" => (
            "SERVER_JS",
            filename.trim_end_matches(".js").trim_end_matches(".gs"),
        ),
        "html" => ("HTML", filename.trim_end_matches(".html")),
        "json" => {
            if filename == "appsscript.json" {
                ("JSON", "appsscript")
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };

    let content = fs::read_to_string(path).map_err(|e| {
        GwsError::Validation(format!("Failed to read file '{}': {}", path.display(), e))
    })?;

    Ok(Some(json!({
        "name": name_val,
        "type": type_val,
        "source": content
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_process_file_server_js() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("code.gs");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "function foo() {{}}").unwrap();

        let result = process_file(&file_path).unwrap().unwrap();
        assert_eq!(result["name"], "code");
        assert_eq!(result["type"], "SERVER_JS");
        assert_eq!(
            result["source"].as_str().unwrap().trim(),
            "function foo() {}"
        );
    }

    #[test]
    fn test_process_file_html() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("index.html");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "<html></html>").unwrap();

        let result = process_file(&file_path).unwrap().unwrap();
        assert_eq!(result["name"], "index");
        assert_eq!(result["type"], "HTML");
    }

    #[test]
    fn test_process_file_appsscript_json() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("appsscript.json");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{{}}").unwrap();

        let result = process_file(&file_path).unwrap().unwrap();
        assert_eq!(result["name"], "appsscript");
        assert_eq!(result["type"], "JSON");
    }

    #[test]
    fn test_process_file_ignored() {
        let dir = tempdir().unwrap();

        // Random JSON
        let p1 = dir.path().join("other.json");
        File::create(&p1).unwrap();
        assert!(process_file(&p1).unwrap().is_none());

        // Hidden file
        let p2 = dir.path().join(".hidden.gs");
        File::create(&p2).unwrap();
        assert!(process_file(&p2).unwrap().is_none());

        // node_modules
        let node_modules = dir.path().join("node_modules");
        fs::create_dir(&node_modules).unwrap();
        let p3 = node_modules.join("dep.gs");
        File::create(&p3).unwrap();
        assert!(process_file(&p3).unwrap().is_none());
    }

    #[test]
    fn test_visit_dirs() {
        let dir = tempdir().unwrap();

        // Root file
        let f1 = dir.path().join("root.gs");
        File::create(&f1).unwrap();

        // Subdir file
        let sub = dir.path().join("src");
        fs::create_dir(&sub).unwrap();
        let f2 = sub.join("utils.js");
        File::create(&f2).unwrap();

        // Ignored file
        let f3 = dir.path().join("ignore.txt");
        File::create(&f3).unwrap();

        let mut files = Vec::new();
        visit_dirs(dir.path(), &mut files).unwrap();

        assert_eq!(files.len(), 2);

        let names: Vec<&str> = files.iter().map(|f| f["name"].as_str().unwrap()).collect();

        assert!(names.contains(&"root"));
        assert!(names.contains(&"utils"));
    }
}

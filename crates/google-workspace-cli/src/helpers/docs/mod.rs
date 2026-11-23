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

pub(crate) mod export;
pub(crate) mod json_to_md;
pub(crate) mod write;

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::executor;
use crate::helpers::drive::api::{DriveClient, DRIVE_SCOPE};
use crate::helpers::drive::print_result;
use clap::{Arg, ArgGroup, ArgMatches, Command};
use serde_json::json;
use std::future::Future;
use std::io::{Read, Write};
use std::pin::Pin;

pub struct DocsHelper;

impl Helper for DocsHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+write")
                .about("[Helper] Write Markdown into a Doc as real formatting (or append plain text)")
                .arg(
                    Arg::new("file")
                        .help("Markdown file to write ('-' reads stdin)")
                        .index(1),
                )
                .arg(
                    Arg::new("document")
                        .long("document")
                        .visible_alias("doc")
                        .help("Existing Doc: id, URL or Drive path. With a Markdown file, its body is replaced")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("new")
                        .long("new")
                        .help("Create a new Doc with this title")
                        .value_name("TITLE")
                        .conflicts_with("document"),
                )
                .arg(
                    Arg::new("folder")
                        .long("folder")
                        .help("Folder for --new: id, URL or Drive path (default: My Drive)")
                        .value_name("FOLDER")
                        .requires("new"),
                )
                .arg(
                    Arg::new("text")
                        .long("text")
                        .help("Append this plain text to --document instead of writing Markdown")
                        .value_name("TEXT")
                        .conflicts_with_all(["file", "new", "folder"]),
                )
                .group(
                    ArgGroup::new("source")
                        .args(["file", "text"])
                        .required(true),
                )
                .group(
                    ArgGroup::new("destination")
                        .args(["document", "new"])
                        .required(true),
                )
                .after_help(
                    "\
EXAMPLES:
  gws docs +write plan.md --new 'Q1 plan'
  gws docs +write plan.md --new 'Q1 plan' --folder 'Company/strategy'
  gws docs +write plan.md --doc DOC_ID
  gws docs +write plan.md --doc 'Company/strategy/Q1 plan'
  cat notes.md | gws docs +write - --doc https://docs.google.com/document/d/ID/edit
  gws docs +write --document DOC_ID --text 'Appended line'

TIPS:
  Headings, nested lists, bold, italic, strikethrough, links, code (as
  monospace), tables and images become real Docs formatting; Drive's own
  Markdown converter does the conversion.
  --doc replaces the body of the existing Doc in place: its id, link,
  sharing and version history stay.
  Local images (relative to the Markdown file) are embedded; remote image
  URLs must be publicly reachable.
  --text keeps the old behaviour: it appends plain text to --document.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+export")
                .about("[Helper] Export a Doc as Markdown, plain text or HTML")
                .arg(
                    Arg::new("doc")
                        .help("Doc id, URL or Drive path")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("output")
                        .long("output")
                        .short('o')
                        .help("Write to this file instead of stdout")
                        .value_name("FILE"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws docs +export DOC_ID
  gws docs +export 'Company/strategy/Q1 plan' --output plan.md
  gws docs +export https://docs.google.com/document/d/ID/edit --format txt
  gws docs +export DOC_ID --format html -o doc.html

TIPS:
  --format takes md (default), txt or html.
  Markdown keeps headings, lists, bold, italic, links, tables and code.
  Images become links: with --output they are saved next to the file in
  <name>_images/; on stdout they link to the image URLs Docs reports, which
  expire after a while.
  Documents too large for Drive's export (10 MB) are converted from the
  Docs API instead.",
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
            if let Some(matches) = matches.subcommand_matches("+export") {
                handle_export(matches).await?;
                return Ok(true);
            }
            if let Some(matches) = matches.subcommand_matches("+write") {
                if matches.get_one::<String>("text").is_none() {
                    handle_write_markdown(matches).await?;
                    return Ok(true);
                }
                let (params_str, body_str, scopes) = build_write_request(matches, doc)?;

                let scope_strs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scope_strs).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) if matches.get_flag("dry-run") => (None, executor::AuthMethod::None),
                    Err(e) => return Err(GwsError::Auth(format!("Docs auth failed: {e}"))),
                };

                // Method: documents.batchUpdate
                let documents_res = doc.resources.get("documents").ok_or_else(|| {
                    GwsError::Discovery("Resource 'documents' not found".to_string())
                })?;
                let batch_update_method =
                    documents_res.methods.get("batchUpdate").ok_or_else(|| {
                        GwsError::Discovery("Method 'documents.batchUpdate' not found".to_string())
                    })?;

                let pagination = executor::PaginationConfig {
                    page_all: false,
                    page_limit: 10,
                    page_delay_ms: 100,
                };

                executor::execute_method(
                    doc,
                    batch_update_method,
                    Some(&params_str),
                    Some(&body_str),
                    token.as_deref(),
                    auth_method,
                    None,
                    None,
                    matches.get_flag("dry-run"),
                    &pagination,
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

async fn handle_export(matches: &ArgMatches) -> Result<(), GwsError> {
    let input = matches.get_one::<String>("doc").unwrap();
    let format =
        export::ExportFormat::parse(matches.get_one::<String>("format").map(String::as_str))?;
    let output = match matches.get_one::<String>("output") {
        Some(p) => Some(crate::validate::validate_safe_file_path(p, "--output")?),
        None => None,
    };
    if matches.get_flag("dry-run") {
        print_result(
            matches,
            &json!({ "dryRun": true, "doc": input, "format": format!("{format:?}").to_lowercase(),
                     "output": output.as_ref().map(|p| p.display().to_string()) }),
        );
        return Ok(());
    }
    let client = DriveClient::connect(&[DRIVE_SCOPE, export::DOCS_READONLY_SCOPE]).await?;
    let result = export::run_export(&client, input, format, output.as_deref()).await?;
    match &output {
        Some(path) => {
            std::fs::write(path, &result.content).map_err(|e| {
                GwsError::Validation(format!("Failed to write '{}': {e}", path.display()))
            })?;
            eprintln!(
                "Exported '{}' to {}{}{}",
                crate::output::sanitize_for_terminal(&result.name),
                path.display(),
                if result.images > 0 {
                    format!(
                        " ({} images in {})",
                        result.images,
                        export::images_dir_for(path).display()
                    )
                } else {
                    String::new()
                },
                if result.converted_from_json {
                    " (converted from the Docs API)"
                } else {
                    ""
                }
            );
            print_result(
                matches,
                &json!({
                    "documentId": result.document_id,
                    "name": result.name,
                    "output": path.display().to_string(),
                    "bytes": result.content.len(),
                    "images": result.images,
                    "convertedFromDocsApi": result.converted_from_json,
                }),
            );
        }
        None => {
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(&result.content)
                .and_then(|_| stdout.flush())
                .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to write output: {e}")))?;
        }
    }
    Ok(())
}

async fn handle_write_markdown(matches: &ArgMatches) -> Result<(), GwsError> {
    let file = matches.get_one::<String>("file").unwrap();
    let (source, base_dir) = if file == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| GwsError::Validation(format!("Failed to read stdin: {e}")))?;
        (
            s,
            std::env::current_dir().map_err(|e| {
                GwsError::Validation(format!("Failed to determine current directory: {e}"))
            })?,
        )
    } else {
        let path = crate::validate::validate_safe_file_path(file, "<file>")?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| GwsError::Validation(format!("Failed to read '{file}': {e}")))?;
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        (text, dir)
    };

    let prepared = write::inline_local_images(&source, |rel| {
        let joined = base_dir.join(rel);
        let joined = joined
            .to_str()
            .ok_or_else(|| GwsError::Validation(format!("Image path is not valid UTF-8: {rel}")))?;
        let safe = crate::validate::validate_safe_file_path(joined, "image")?;
        std::fs::read(&safe)
            .map_err(|e| GwsError::Validation(format!("Failed to read image '{rel}': {e}")))
    })?;
    for w in &prepared.warnings {
        eprintln!("warning: {}", crate::output::sanitize_for_terminal(w));
    }

    let destination = match (
        matches.get_one::<String>("document"),
        matches.get_one::<String>("new"),
    ) {
        (Some(doc), _) => write::Destination::Existing(doc),
        (None, Some(title)) => {
            crate::validate::reject_dangerous_chars(title, "--new")?;
            write::Destination::New {
                title,
                folder: matches.get_one::<String>("folder").map(String::as_str),
            }
        }
        (None, None) => unreachable!("clap requires --document or --new"),
    };

    if matches.get_flag("dry-run") {
        let (action, target) = match &destination {
            write::Destination::Existing(d) => ("replace", json!({ "document": d })),
            write::Destination::New { title, folder } => {
                ("create", json!({ "title": title, "folder": folder }))
            }
        };
        print_result(
            matches,
            &json!({
                "dryRun": true,
                "action": action,
                "target": target,
                "bytes": prepared.markdown.len(),
                "imagesInlined": prepared.images_inlined,
            }),
        );
        return Ok(());
    }

    let client = DriveClient::connect(&[DRIVE_SCOPE]).await?;
    let (file, action) = write::run_write(&client, &prepared, destination).await?;
    let url = file
        .web_view_link
        .clone()
        .unwrap_or_else(|| format!("https://docs.google.com/document/d/{}/edit", file.id));
    eprintln!(
        "{} '{}': {url}",
        if action == "created" {
            "Created"
        } else {
            "Replaced the body of"
        },
        crate::output::sanitize_for_terminal(&file.name)
    );
    print_result(
        matches,
        &json!({
            "documentId": file.id,
            "name": file.name,
            "url": url,
            "action": action,
            "imagesInlined": prepared.images_inlined,
        }),
    );
    Ok(())
}

fn build_write_request(
    matches: &ArgMatches,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let document_id = matches.get_one::<String>("document").unwrap();
    let text = matches.get_one::<String>("text").unwrap();

    let documents_res = doc
        .resources
        .get("documents")
        .ok_or_else(|| GwsError::Discovery("Resource 'documents' not found".to_string()))?;
    let batch_update_method = documents_res.methods.get("batchUpdate").ok_or_else(|| {
        GwsError::Discovery("Method 'documents.batchUpdate' not found".to_string())
    })?;

    let params = json!({
        "documentId": document_id
    });

    let body = json!({
        "requests": [
            {
                "insertText": {
                    "text": text,
                    "endOfSegmentLocation": {
                        "segmentId": "" // Empty means body
                    }
                }
            }
        ]
    });

    let scopes: Vec<String> = batch_update_method
        .scopes
        .iter()
        .map(|s| s.to_string())
        .collect();

    Ok((params.to_string(), body.to_string(), scopes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{RestDescription, RestMethod, RestResource};
    use std::collections::HashMap;

    fn make_mock_doc() -> RestDescription {
        let mut methods = HashMap::new();
        methods.insert(
            "batchUpdate".to_string(),
            RestMethod {
                scopes: vec!["https://scope".to_string()],
                ..Default::default()
            },
        );

        let mut documents_res = RestResource::default();
        documents_res.methods = methods;

        let mut resources = HashMap::new();
        resources.insert("documents".to_string(), documents_res);

        RestDescription {
            resources,
            ..Default::default()
        }
    }

    fn make_matches_write(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("document").long("document"))
            .arg(Arg::new("text").long("text"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_build_write_request() {
        let doc = make_mock_doc();
        let matches = make_matches_write(&["test", "--document", "123", "--text", "hello world"]);
        let (params, body, scopes) = build_write_request(&matches, &doc).unwrap();

        assert!(params.contains("123"));
        assert!(body.contains("hello world"));
        assert!(body.contains("endOfSegmentLocation"));
        assert_eq!(scopes[0], "https://scope");
    }
}

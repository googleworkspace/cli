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
use clap::{Arg, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
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
                .about("[Helper] Append text to a document")
                .arg(
                    Arg::new("document")
                        .long("document")
                        .help("Document ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("text")
                        .long("text")
                        .help("Text to append (plain text)")
                        .required(true)
                        .value_name("TEXT"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws docs +write --document DOC_ID --text 'Hello, world!'

TIPS:
  Text is inserted at the end of the document body.
  For rich formatting, use the raw batchUpdate API instead.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+revisions")
                .about("[Helper] List revision history of a document")
                .arg(
                    Arg::new("document")
                        .long("document")
                        .help("Document ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum number of revisions to return (default: 20)")
                        .value_name("N")
                        .value_parser(clap::value_parser!(u32).range(1..=1000)),
                )
                .after_help(
                    "\
EXAMPLES:
  gws docs +revisions --document DOC_ID
  gws docs +revisions --document DOC_ID --limit 5
  gws docs +revisions --document DOC_ID --format table

TIPS:
  The document ID is the long string in the Google Docs URL.
  Returns metadata for each revision: ID, modified time, author, and
  whether the revision is kept forever.
  Note: the full content of past revisions is not accessible via the
  Google API for native Docs files. Use the Google Docs UI (File →
  Version history) to view or restore specific versions.",
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
            if let Some(matches) = matches.subcommand_matches("+write") {
                let (params_str, body_str, scopes) = build_write_request(matches, doc)?;

                let scope_strs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scope_strs).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) if matches.get_flag("dry-run") => (None, executor::AuthMethod::None),
                    Err(e) => {
                        return Err(GwsError::Auth(format!(
                            "Docs auth failed: {}",
                            crate::output::sanitize_for_terminal(&e.to_string())
                        )))
                    }
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

            if let Some(matches) = matches.subcommand_matches("+revisions") {
                handle_revisions(matches).await?;
                return Ok(true);
            }

            Ok(false)
        })
    }
}

const REVISION_FIELDS: &str =
    "revisions(id,modifiedTime,lastModifyingUser/displayName,keepForever,size)";

struct RevisionsRequest {
    url: String,
    limit_str: String,
}

fn build_revisions_request(document_id: &str, limit: u32) -> RevisionsRequest {
    let encoded_id =
        percent_encoding::utf8_percent_encode(document_id, percent_encoding::NON_ALPHANUMERIC);
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{}/revisions",
        encoded_id
    );
    RevisionsRequest {
        url,
        limit_str: limit.to_string(),
    }
}

async fn handle_revisions(matches: &ArgMatches) -> Result<(), GwsError> {
    let document_id = matches.get_one::<String>("document").unwrap();
    let limit = matches.get_one::<u32>("limit").copied().unwrap_or(20);
    let dry_run = matches.get_flag("dry-run");

    let scope = "https://www.googleapis.com/auth/drive.readonly";
    let token = if dry_run {
        None
    } else {
        Some(auth::get_token(&[scope]).await.map_err(|e| {
            GwsError::Auth(format!(
                "Docs auth failed: {}",
                crate::output::sanitize_for_terminal(&e.to_string())
            ))
        })?)
    };

    let req = build_revisions_request(document_id, limit);
    let url = req.url;
    let limit_str = req.limit_str;

    if dry_run {
        let dry_run_info = json!({
            "dry_run": true,
            "url": url,
            "method": "GET",
            "query_params": {
                "fields": REVISION_FIELDS,
                "pageSize": limit_str,
            },
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&dry_run_info).unwrap_or_default()
        );
        return Ok(());
    }

    let token = token.unwrap(); // safe: dry_run path already returned above
    let client = crate::client::build_client()?;
    let resp = crate::client::send_with_retry(|| {
        client
            .get(url.clone())
            .query(&[
                ("fields", REVISION_FIELDS),
                ("pageSize", limit_str.as_str()),
            ])
            .bearer_auth(token.clone())
    })
    .await
    .map_err(|e| GwsError::Other(anyhow::anyhow!("HTTP request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("Failed to read error response body: {e}"));
        return Err(GwsError::from_api_response(status.as_u16(), &body));
    }

    let value: Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("JSON parse failed: {e}")))?;

    let fmt = matches
        .get_one::<String>("format")
        .map(|s| crate::formatter::OutputFormat::from_str(s))
        .unwrap_or_default();
    println!("{}", crate::formatter::format_value(&value, &fmt));
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

    #[test]
    fn test_revisions_command_registered() {
        let helper = DocsHelper;
        let base = Command::new("docs");
        let doc = RestDescription::default();
        let cmd = helper.inject_commands(base, &doc);
        let subcommands: Vec<&str> = cmd.get_subcommands().map(|c| c.get_name()).collect();
        assert!(subcommands.contains(&"+revisions"));
        assert!(subcommands.contains(&"+write"));
    }

    #[test]
    fn test_revisions_requires_document() {
        let helper = DocsHelper;
        let base = Command::new("docs");
        let doc = RestDescription::default();
        let cmd = helper.inject_commands(base, &doc);
        let revisions_cmd = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "+revisions")
            .unwrap();
        let doc_arg = revisions_cmd
            .get_arguments()
            .find(|a| a.get_id() == "document")
            .unwrap();
        assert!(doc_arg.is_required_set());
    }

    #[test]
    fn test_build_revisions_request_url() {
        let req = build_revisions_request("abc123", 20);
        assert_eq!(
            req.url,
            "https://www.googleapis.com/drive/v3/files/abc123/revisions"
        );
        assert_eq!(req.limit_str, "20");
    }

    #[test]
    fn test_build_revisions_request_percent_encodes_document_id() {
        let req = build_revisions_request("abc/def 123", 5);
        // slash and space in document ID must be percent-encoded
        assert!(
            req.url.contains("abc%2Fdef%20123"),
            "document id should be percent-encoded in URL: {}",
            req.url
        );
        // the raw unencoded document ID must not appear
        assert!(
            !req.url.contains("abc/def 123"),
            "raw document id must not appear unencoded in URL: {}",
            req.url
        );
    }

    #[test]
    fn test_build_revisions_request_limit_str() {
        let req = build_revisions_request("docid", 42);
        assert_eq!(req.limit_str, "42");
    }

    #[test]
    fn test_revisions_dry_run_output_structure() {
        let req = build_revisions_request("testdoc", 10);
        let dry_run_info = serde_json::json!({
            "dry_run": true,
            "url": req.url,
            "method": "GET",
            "query_params": {
                "fields": REVISION_FIELDS,
                "pageSize": req.limit_str,
            },
        });
        assert_eq!(dry_run_info["dry_run"], true);
        assert!(dry_run_info["url"].as_str().unwrap().contains("testdoc"));
        assert_eq!(dry_run_info["method"], "GET");
        assert!(dry_run_info["query_params"]["fields"]
            .as_str()
            .unwrap()
            .contains("revisions"));
        assert_eq!(dry_run_info["query_params"]["pageSize"], "10");
    }
}

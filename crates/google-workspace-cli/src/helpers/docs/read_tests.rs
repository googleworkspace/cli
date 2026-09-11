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

use crate::commands::build_cli;
use crate::discovery::RestDescription;
use crate::error::GwsError;
use crate::helpers::modelarmor::{SanitizeConfig, SanitizeMode};
use serde_json::{json, Value};

use super::read;

fn discovery() -> RestDescription {
    serde_json::from_value(serde_json::json!({
        "name": "docs", "version": "v1", "rootUrl": "https://docs.example.invalid/",
        "servicePath": "v1/",
        "resources": {"documents": {"methods": {"get": {
            "id": "docs.documents.get", "httpMethod": "GET",
            "path": "documents/{documentId}", "parameterOrder": ["documentId"],
            "parameters": {"documentId": {"type": "string", "location": "path", "required": true}},
            "scopes": ["https://www.googleapis.com/auth/documents.readonly"]
        }}}}
    }))
    .unwrap()
}

#[test]
fn registers_read_alongside_write_with_required_document() {
    let cli = build_cli(&discovery());
    assert!(cli.find_subcommand("+write").is_some());
    assert!(
        cli.find_subcommand("+read").is_some(),
        "missing structured reader"
    );
    let error = cli
        .clone()
        .try_get_matches_from(["gws", "+read"])
        .unwrap_err();
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    assert!(cli
        .try_get_matches_from([
            "gws",
            "+read",
            "--document",
            "synthetic",
            "--params",
            "{}",
            "--format",
            "yaml",
            "--dry-run"
        ])
        .is_ok());
}

fn matches(args: &[&str]) -> clap::ArgMatches {
    build_cli(&discovery()).try_get_matches_from(args).unwrap()
}

fn paragraph(text: &str) -> Value {
    json!({"startIndex": 1, "endIndex": 5, "paragraph": {
        "elements": [{"startIndex": 1, "endIndex": 5, "textRun": {"content": text}}]
    }})
}

fn legacy() -> Value {
    json!({
        "documentId": "synthetic", "title": "Example", "revisionId": "rev-1",
        "suggestionsViewMode": "SUGGESTIONS_INLINE",
        "body": {"content": [paragraph("Hi😀")] }
    })
}

#[test]
fn request_requires_full_inline_tabs_and_preserves_other_params() {
    let params =
        read::build_params("synthetic", Some(r#"{"fields":"*","prettyPrint":false}"#)).unwrap();
    assert_eq!(
        params,
        json!({
            "documentId": "synthetic", "fields": "*", "prettyPrint": false,
            "includeTabsContent": true, "suggestionsViewMode": "SUGGESTIONS_INLINE"
        })
    );
    assert_eq!(
        read::build_params("id", None).unwrap()["includeTabsContent"],
        true
    );
    assert!(read::build_params("id", Some(
        r#"{"includeTabsContent":true,"suggestionsViewMode":"SUGGESTIONS_INLINE","documentId":"id"}"#
    )).is_ok());
}

#[test]
fn request_rejects_partial_masks_lossy_views_and_parameter_bypasses() {
    for params in [
        r#"{"fields":"title"}"#,
        r#"{"fields":"tabs(documentTab/body/content)"}"#,
        r#"{"fields":""}"#,
        r#"{"fields":null}"#,
        r#"{"fields": ["*"]}"#,
        r#"{"includeTabsContent":false}"#,
        r#"{"includeTabsContent":"true"}"#,
        r#"{"suggestionsViewMode":"PREVIEW_WITHOUT_SUGGESTIONS"}"#,
        r#"{"suggestionsViewMode":"DEFAULT_FOR_CURRENT_ACCESS"}"#,
        r#"{"documentId":"other"}"#,
        r#"{"alt":"media"}"#,
        r#"{"$fields":"title"}"#,
        "[]",
        "null",
        "{",
    ] {
        assert!(read::build_params("id", Some(params)).is_err(), "{params}");
    }
    for id in [
        "",
        "../../secret",
        "id?fields=title",
        "id#fragment",
        "id\n",
        "%2e%2e",
    ] {
        assert!(read::build_params(id, None).is_err(), "{id:?}");
    }
}

#[test]
fn legacy_body_keeps_source_revision_text_and_utf16_indices() {
    let output = read::normalize(&legacy()).unwrap();
    assert_eq!(output["documentId"], "synthetic");
    assert_eq!(output["revisionId"], "rev-1");
    assert_eq!(output["suggestionsViewMode"], "SUGGESTIONS_INLINE");
    assert_eq!(output["source"], "legacyBody");
    assert_eq!(output["tabs"][0]["tabId"], Value::Null);
    let block = &output["tabs"][0]["blocks"][0];
    assert_eq!(block["type"], "paragraph");
    assert_eq!(block["text"], "Hi😀");
    assert_eq!(block["endIndex"], 5);
    assert_eq!(block["elements"][0]["endIndex"], 5);
    assert_eq!(block["elements"][0]["text"], "Hi😀");
}

#[test]
fn recursively_reads_tabs_and_child_tabs_in_api_order_without_duplicate_legacy_body() {
    let mut input = legacy();
    input["tabs"] = json!([
        {"tabProperties": {"tabId": "a", "title": "First", "index": 0},
         "documentTab": {"body": {"content": [paragraph("first")]}},
         "childTabs": [{"tabProperties": {"tabId": "b", "title": "Child", "parentTabId": "a"},
             "documentTab": {"body": {"content": [paragraph("child")]}},
             "childTabs": [{"tabProperties": {"tabId": "c", "title": "Grandchild"},
                 "documentTab": {"body": {"content": [paragraph("grandchild")]}}}]}]},
        {"tabProperties": {"tabId": "d", "title": "Last", "index": 1},
         "documentTab": {"body": {"content": [paragraph("last")]}}}
    ]);
    let output = read::normalize(&input).unwrap();
    assert_eq!(output["source"], "tabs");
    assert_eq!(output["tabs"].as_array().unwrap().len(), 2);
    assert_eq!(output["tabs"][0]["blocks"][0]["text"], "first");
    assert_eq!(output["tabs"][0]["childTabs"][0]["parentTabId"], "a");
    assert_eq!(
        output["tabs"][0]["childTabs"][0]["childTabs"][0]["parentTabId"],
        "b"
    );
    assert_eq!(output["tabs"][1]["tabId"], "d");
    input["tabs"] = json!([]);
    assert_eq!(read::normalize(&input).unwrap()["source"], "legacyBody");
}

#[test]
fn preserves_styled_link_runs_and_inline_suggestion_metadata_in_outline_order() {
    let input = json!({"documentId": "id", "title": "Styled", "body": {"content": [{
        "startIndex": 1, "endIndex": 9, "paragraph": {
            "paragraphStyle": {"namedStyleType": "HEADING_2", "headingId": "h1"},
            "suggestedParagraphStyleChanges": {"s1": {"paragraphStyle": {"namedStyleType": "HEADING_1"}}},
            "elements": [
                {"startIndex": 1, "endIndex": 5, "textRun": {
                    "content": "Look", "textStyle": {"bold": true, "link": {"url": "https://example.invalid"}},
                    "suggestedInsertionIds": ["s1"],
                    "suggestedTextStyleChanges": {"s2": {"textStyle": {"italic": true}}}}},
                {"startIndex": 5, "endIndex": 9, "textRun": {
                    "content": "here", "textStyle": {"italic": true, "link": {"heading": {"id": "h2", "tabId": "t2"}}},
                    "suggestedDeletionIds": ["s3"]}}
            ]
        }
    }, {"startIndex": 9, "endIndex": 14, "paragraph": {
        "paragraphStyle": {"namedStyleType": "TITLE"}, "elements": []
    }}]}});
    let output = read::normalize(&input).unwrap();
    let block = &output["tabs"][0]["blocks"][0];
    assert_eq!(block["text"], "Lookhere");
    assert_eq!(block["paragraphStyle"]["headingId"], "h1");
    assert!(block["suggestedParagraphStyleChanges"]["s1"].is_object());
    assert_eq!(block["elements"][0]["textStyle"]["bold"], true);
    assert_eq!(
        block["elements"][0]["textStyle"]["link"]["url"],
        "https://example.invalid"
    );
    assert_eq!(block["elements"][0]["suggestedInsertionIds"], json!(["s1"]));
    assert_eq!(block["elements"][1]["suggestedDeletionIds"], json!(["s3"]));
    assert_eq!(
        block["elements"][1]["textStyle"]["link"]["heading"]["tabId"],
        "t2"
    );
    assert!(block["elements"][0]["suggestedTextStyleChanges"]["s2"].is_object());
    assert_eq!(
        output["outline"][0],
        json!({
            "tabId": null, "level": "HEADING_2", "headingId": "h1", "text": "Lookhere",
            "startIndex": 1, "endIndex": 9, "path": "/tabs/0/blocks/0"
        })
    );
    assert_eq!(output["outline"][1]["level"], "TITLE");
}

#[test]
fn nested_tables_keep_row_cell_order_indices_styles_and_suggestions() {
    let input = json!({"documentId": "id", "body": {"content": [{
        "startIndex": 10, "endIndex": 30, "table": {"rows": 1, "columns": 2,
            "tableRows": [{"startIndex": 11, "endIndex": 29, "tableCells": [
                {"startIndex": 12, "endIndex": 25, "tableCellStyle": {"rowSpan": 1, "columnSpan": 1},
                 "suggestedInsertionIds": ["cell-s"], "content": [
                    paragraph("cell"),
                    {"startIndex": 17, "endIndex": 24, "table": {"rows": 1, "columns": 1,
                        "tableRows": [{"tableCells": [{"content": [paragraph("nested")]}]}]}}
                 ]},
                {"content": [paragraph("second")]}
            ]}]
        }
    }, paragraph("after")]}});
    let output = read::normalize(&input).unwrap();
    let table = &output["tabs"][0]["blocks"][0];
    assert_eq!(table["type"], "table");
    assert_eq!(table["startIndex"], 10);
    assert_eq!(table["rowCount"], 1);
    assert_eq!(table["columns"], 2);
    assert_eq!(table["rows"][0]["endIndex"], 29);
    let cell = &table["rows"][0]["cells"][0];
    assert_eq!(cell["startIndex"], 12);
    assert_eq!(cell["suggestedInsertionIds"], json!(["cell-s"]));
    assert_eq!(cell["tableCellStyle"]["columnSpan"], 1);
    assert_eq!(cell["blocks"][0]["text"], "cell");
    assert_eq!(
        cell["blocks"][1]["rows"][0]["cells"][0]["blocks"][0]["text"],
        "nested"
    );
    assert_eq!(table["rows"][0]["cells"][1]["blocks"][0]["text"], "second");
    assert_eq!(output["tabs"][0]["blocks"][1]["text"], "after");
}

#[test]
fn figures_keep_references_and_metadata_even_without_content_uri() {
    let mut input = legacy();
    input["body"]["content"][0]["paragraph"]["elements"] = json!([
        {"startIndex": 1, "endIndex": 2, "inlineObjectElement": {
            "inlineObjectId": "image", "suggestedInsertionIds": ["s1"], "textStyle": {"baselineOffset": "SUPERSCRIPT"}}},
        {"startIndex": 2, "endIndex": 3, "inlineObjectElement": {"inlineObjectId": "missing"}}
    ]);
    input["body"]["content"][0]["paragraph"]["positionedObjectIds"] = json!(["drawing"]);
    input["inlineObjects"] = json!({"image": {
        "objectId": "image", "inlineObjectProperties": {"embeddedObject": {
            "title": "Alt title", "description": "Alt text", "size": {"width": {"magnitude": 42, "unit": "PT"}},
            "imageProperties": {"sourceUri": "https://example.invalid/image.png"}
        }}, "suggestedDeletionIds": ["s2"]
    }});
    input["positionedObjects"] = json!({"drawing": {
        "objectId": "drawing", "positionedObjectProperties": {"embeddedObject": {"embeddedDrawingProperties": {}}}
    }});
    let output = read::normalize(&input).unwrap();
    let tab = &output["tabs"][0];
    assert_eq!(tab["blocks"][0]["elements"][0]["type"], "figure");
    assert_eq!(tab["blocks"][0]["elements"][0]["objectId"], "image");
    assert_eq!(
        tab["blocks"][0]["elements"][0]["suggestedInsertionIds"],
        json!(["s1"])
    );
    assert_eq!(tab["blocks"][0]["elements"][1]["objectId"], "missing");
    assert_eq!(tab["blocks"][0]["positionedObjectIds"], json!(["drawing"]));
    assert_eq!(tab["figures"]["image"]["type"], "image");
    assert_eq!(
        tab["figures"]["image"]["embeddedObject"]["description"],
        "Alt text"
    );
    assert!(tab["figures"]["image"]["embeddedObject"]["imageProperties"]
        .get("contentUri")
        .is_none());
    assert_eq!(
        tab["figures"]["image"]["suggestedDeletionIds"],
        json!(["s2"])
    );
    assert_eq!(tab["figures"]["drawing"]["placement"], "positioned");
}

#[test]
fn preserves_reference_markers_segments_unknown_blocks_and_unknown_inline_elements() {
    let mut input = legacy();
    input["body"]["content"] = json!([
        {"endIndex": 1, "sectionBreak": {"sectionStyle": {"columnSeparatorStyle": "NONE"}}},
        {"startIndex": 1, "endIndex": 4, "paragraph": {"elements": [
            {"startIndex": 1, "endIndex": 2, "footnoteReference": {"footnoteId": "f1", "footnoteNumber": "1"}},
            {"startIndex": 2, "endIndex": 3, "person": {"personId": "p1"}},
            {"startIndex": 3, "endIndex": 4, "futureInline": {"label": "unrecognized"}}
        ]}},
        {"startIndex": 4, "endIndex": 8, "futureBlock": {"content": "keep me"}},
        {"tableOfContents": {"content": [paragraph("toc")]}}
    ]);
    input["headers"] = json!({"h1": {"headerId": "h1", "content": [paragraph("header")]}});
    input["footers"] = json!({"f2": {"footerId": "f2", "content": [paragraph("footer")]}});
    input["footnotes"] = json!({"f1": {"footnoteId": "f1", "content": [paragraph("note")]}});
    input["namedStyles"] = json!({"styles": [{"namedStyleType": "NORMAL_TEXT"}]});
    let output = read::normalize(&input).unwrap();
    let tab = &output["tabs"][0];
    assert_eq!(tab["blocks"][0]["type"], "sectionBreak");
    assert!(tab["blocks"][0].get("startIndex").is_none());
    assert_eq!(tab["blocks"][1]["elements"][0]["type"], "footnoteReference");
    assert_eq!(tab["blocks"][1]["elements"][0]["footnoteId"], "f1");
    assert_eq!(tab["blocks"][1]["elements"][1]["type"], "unknown");
    assert_eq!(
        tab["blocks"][1]["elements"][1]["data"]["person"]["personId"],
        "p1"
    );
    assert_eq!(
        tab["blocks"][1]["elements"][2]["data"]["futureInline"]["label"],
        "unrecognized"
    );
    assert_eq!(tab["blocks"][2]["type"], "unknown");
    assert_eq!(tab["blocks"][2]["startIndex"], 4);
    assert_eq!(
        tab["blocks"][2]["data"]["futureBlock"]["content"],
        "keep me"
    );
    assert_eq!(tab["blocks"][3]["blocks"][0]["text"], "toc");
    assert_eq!(tab["headers"]["h1"]["blocks"][0]["text"], "header");
    assert_eq!(tab["footers"]["f2"]["blocks"][0]["text"], "footer");
    assert_eq!(tab["footnotes"]["f1"]["blocks"][0]["text"], "note");
    assert_eq!(
        tab["namedStyles"]["styles"][0]["namedStyleType"],
        "NORMAL_TEXT"
    );
}

#[test]
fn rejects_missing_content_instead_of_claiming_empty_document() {
    for input in [
        json!({"documentId": "id", "title": "metadata only"}),
        json!({"body": {}}),
        json!({"tabs": [{"tabProperties": {"tabId": "t"}, "documentTab": {}}]}),
        json!({"tabs": [{"documentTab": {"body": {"content": "not an array"}}}]}),
        json!({"tabs": "not an array", "body": {"content": []}}),
        json!({"body": {"content": [{"table": {"rows": 1}}]}}),
    ] {
        assert!(read::normalize(&input).is_err(), "{input}");
    }
    assert!(read::normalize(&json!({"body": {"content": []}})).is_ok());
}

#[test]
fn preserves_unknown_tab_and_sanitization_annotation() {
    let output = read::normalize(&json!({
        "documentId": "id", "_sanitization": {"filterMatchState": "NO_MATCH_FOUND"},
        "tabs": [{"tabProperties": {"tabId": "future", "title": "Future"},
                  "futureTab": {"content": "opaque"}}]
    }))
    .unwrap();
    assert_eq!(
        output["_sanitization"]["filterMatchState"],
        "NO_MATCH_FOUND"
    );
    assert_eq!(output["tabs"][0]["type"], "unknown");
    assert_eq!(output["tabs"][0]["data"]["futureTab"]["content"], "opaque");
}

#[tokio::test]
async fn dry_run_uses_executor_plan_without_polling_auth_or_sanitize() {
    let args = matches(&[
        "gws",
        "+read",
        "--document",
        "a/b c",
        "--dry-run",
        "--params",
        r#"{"prettyPrint":false}"#,
    ]);
    let result = read::run(
        &discovery(),
        args.subcommand_matches("+read").unwrap(),
        &SanitizeConfig {
            template: Some("never-call".into()),
            mode: SanitizeMode::Block,
        },
        async { panic!("dry-run must not poll authentication") },
    )
    .await
    .unwrap();
    let output: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(output["dry_run"], true);
    assert_eq!(output["method"], "GET");
    assert_eq!(
        output["url"],
        "https://docs.example.invalid/v1/documents/a%2Fb%20c"
    );
    let query = output["query_params"].as_array().unwrap();
    assert!(query.contains(&json!(["includeTabsContent", "true"])));
    assert!(query.contains(&json!(["suggestionsViewMode", "SUGGESTIONS_INLINE"])));
    assert!(query.contains(&json!(["prettyPrint", "false"])));
    assert!(output.get("tabs").is_none());
}

#[tokio::test]
async fn rejects_partial_mask_before_polling_authentication() {
    let args = matches(&[
        "gws",
        "+read",
        "--document",
        "id",
        "--params",
        r#"{"fields":"title"}"#,
    ]);
    let result = read::run(
        &discovery(),
        args.subcommand_matches("+read").unwrap(),
        &SanitizeConfig::default(),
        async { panic!("invalid request must fail before authentication") },
    )
    .await;
    assert!(matches!(result, Err(GwsError::Validation(_))));
}

#[tokio::test]
async fn propagates_auth_and_discovery_failures() {
    let args = matches(&["gws", "+read", "--document", "id"]);
    let result = read::run(
        &discovery(),
        args.subcommand_matches("+read").unwrap(),
        &SanitizeConfig::default(),
        async { Err(GwsError::Auth("synthetic failure".into())) },
    )
    .await;
    assert!(matches!(result, Err(GwsError::Auth(_))));
    let result = read::run(
        &RestDescription::default(),
        args.subcommand_matches("+read").unwrap(),
        &SanitizeConfig::default(),
        async { panic!("missing method must fail before authentication") },
    )
    .await;
    assert!(matches!(result, Err(GwsError::Discovery(_))));
}

// The transport is the only fake: real executor, request building, capture,
// normalization and formatting run against a loopback server with synthetic auth.
async fn serve(status: &str, body: String) -> (RestDescription, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut doc = discovery();
    doc.root_url = format!("http://{}/", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 1024];
            let len = stream.read(&mut buffer).await.unwrap();
            assert_ne!(len, 0);
            request.extend_from_slice(&buffer[..len]);
            if request.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        stream.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (doc, task)
}

// Stop the existing executor's quota lookup before it can read host config/ADC.
struct SyntheticQuota(Option<std::ffi::OsString>);
impl SyntheticQuota {
    fn new() -> Self {
        let old = std::env::var_os("GOOGLE_WORKSPACE_PROJECT_ID");
        std::env::set_var("GOOGLE_WORKSPACE_PROJECT_ID", "synthetic-project");
        Self(old)
    }
}
impl Drop for SyntheticQuota {
    fn drop(&mut self) {
        if let Some(old) = &self.0 {
            std::env::set_var("GOOGLE_WORKSPACE_PROJECT_ID", old);
        } else {
            std::env::remove_var("GOOGLE_WORKSPACE_PROJECT_ID");
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn executor_fetches_full_content_with_auth_and_honors_all_global_formats() {
    let _quota = SyntheticQuota::new();
    for format in ["json", "yaml", "table", "csv"] {
        let (doc, request) = serve("200 OK", legacy().to_string()).await;
        let args = matches(&[
            "gws",
            "+read",
            "--document",
            "synthetic",
            "--format",
            format,
        ]);
        let rendered = read::run(
            &doc,
            args.subcommand_matches("+read").unwrap(),
            &SanitizeConfig::default(),
            async { Ok("synthetic-token".into()) },
        )
        .await
        .unwrap();
        let request = request.await.unwrap();
        assert!(request.starts_with("GET /v1/documents/synthetic?"));
        assert!(request.contains("includeTabsContent=true"));
        assert!(request.contains("suggestionsViewMode=SUGGESTIONS_INLINE"));
        assert!(request.contains("authorization: Bearer synthetic-token\r\n"));
        match format {
            "json" => assert_eq!(
                serde_json::from_str::<Value>(&rendered).unwrap()["tabs"][0]["blocks"][0]["text"],
                "Hi😀"
            ),
            "yaml" => assert!(rendered.contains("documentId: \"synthetic\"")),
            "table" => assert!(rendered.contains("─") && rendered.contains("blocks")),
            "csv" => assert!(
                rendered.lines().next().unwrap().contains("blocks,")
                    && rendered.contains("\"\"text\"\"")
            ),
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn executor_propagates_server_errors_and_rejects_non_document_responses() {
    let _quota = SyntheticQuota::new();
    for status in ["403 Forbidden", "500 Internal Server Error"] {
        let (doc, request) = serve(
            status,
            json!({"error": {"message": "synthetic denied"}}).to_string(),
        )
        .await;
        let args = matches(&["gws", "+read", "--document", "synthetic"]);
        let result = read::run(
            &doc,
            args.subcommand_matches("+read").unwrap(),
            &SanitizeConfig::default(),
            async { Ok("synthetic-token".into()) },
        )
        .await;
        match result.unwrap_err() {
            GwsError::Api { code, message, .. } => {
                assert_eq!(code, if status.starts_with("403") { 403 } else { 500 });
                assert_eq!(message, "synthetic denied");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        request.await.unwrap();
    }
    for body in [r#"{"documentId":"id","title":"partial"}"#, "invalid JSON"] {
        let (doc, request) = serve("200 OK", body.into()).await;
        let args = matches(&["gws", "+read", "--document", "synthetic"]);
        assert!(read::run(
            &doc,
            args.subcommand_matches("+read").unwrap(),
            &SanitizeConfig::default(),
            async { Ok("synthetic-token".into()) }
        )
        .await
        .is_err());
        request.await.unwrap();
    }
}

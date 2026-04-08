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
use serde_json::json;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// Points per inch (1 inch = 72 typographic points).
const POINTS_PER_INCH: f64 = 72.0;

/// Recognized paper sizes and their portrait dimensions in points (width, height).
const PAPER_SIZES: &[(&str, f64, f64)] = &[
    ("letter", 612.0, 792.0),     // 8.5 × 11"
    ("tabloid", 792.0, 1224.0),   // 11 × 17"
    ("legal", 612.0, 1008.0),     // 8.5 × 14"
    ("statement", 396.0, 612.0),  // 5.5 × 8.5"
    ("executive", 522.0, 756.0),  // 7.25 × 10.5"
    ("folio", 612.0, 936.0),      // 8.5 × 13"
    ("a3", 841.68, 1190.88),      // 11.69 × 16.54"
    ("a4", 595.44, 841.68),       // 8.27 × 11.69"
    ("a5", 419.76, 595.44),       // 5.83 × 8.27"
    ("b4", 708.48, 1000.80),      // 9.84 × 13.90"
    ("b5", 498.96, 708.48),       // 6.93 × 9.84"
];

/// Returns portrait dimensions (width, height) in points for a named paper size.
fn paper_size_points(name: &str) -> Option<(f64, f64)> {
    PAPER_SIZES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, w, h)| (*w, *h))
}

/// Parses a hex color string (with or without `#` prefix) into RGB floats 0.0–1.0.
fn parse_hex_color(hex: &str) -> Result<(f64, f64, f64), GwsError> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return Err(GwsError::Validation(
            "Page color must be a 6-digit hex code (e.g., ff0000 or #ff0000)".to_string(),
        ));
    }
    let r = u8::from_str_radix(&hex[0..2], 16)
        .map_err(|_| GwsError::Validation(format!("Invalid hex color: {hex}")))?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .map_err(|_| GwsError::Validation(format!("Invalid hex color: {hex}")))?;
    let b = u8::from_str_radix(&hex[4..6], 16)
        .map_err(|_| GwsError::Validation(format!("Invalid hex color: {hex}")))?;
    Ok((
        r as f64 / 255.0,
        g as f64 / 255.0,
        b as f64 / 255.0,
    ))
}

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
            Command::new("+page-setup")
                .about("[Helper] Configure page mode, orientation, size, margins, and color")
                .arg(
                    Arg::new("document")
                        .long("document")
                        .help("Document ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("mode")
                        .long("mode")
                        .help("Document mode")
                        .value_parser(["pages", "pageless"])
                        .value_name("MODE"),
                )
                .arg(
                    Arg::new("orientation")
                        .long("orientation")
                        .help("Page orientation (pages mode only)")
                        .value_parser(["portrait", "landscape"])
                        .value_name("ORIENTATION"),
                )
                .arg(
                    Arg::new("paper-size")
                        .long("paper-size")
                        .help("Paper size")
                        .value_parser([
                            "letter", "tabloid", "legal", "statement", "executive",
                            "folio", "a3", "a4", "a5", "b4", "b5",
                        ])
                        .value_name("SIZE"),
                )
                .arg(
                    Arg::new("margin-top")
                        .long("margin-top")
                        .help("Top margin in inches")
                        .value_parser(clap::value_parser!(f64).range(0.0..))
                        .value_name("INCHES"),
                )
                .arg(
                    Arg::new("margin-bottom")
                        .long("margin-bottom")
                        .help("Bottom margin in inches")
                        .value_parser(clap::value_parser!(f64).range(0.0..))
                        .value_name("INCHES"),
                )
                .arg(
                    Arg::new("margin-left")
                        .long("margin-left")
                        .help("Left margin in inches")
                        .value_parser(clap::value_parser!(f64).range(0.0..))
                        .value_name("INCHES"),
                )
                .arg(
                    Arg::new("margin-right")
                        .long("margin-right")
                        .help("Right margin in inches")
                        .value_parser(clap::value_parser!(f64).range(0.0..))
                        .value_name("INCHES"),
                )
                .arg(
                    Arg::new("page-color")
                        .long("page-color")
                        .help("Page background color as hex (e.g., #ffffff)")
                        .value_name("HEX"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws docs +page-setup --document DOC_ID --paper-size a4 --orientation landscape
  gws docs +page-setup --document DOC_ID --margin-top 0.5 --margin-bottom 0.5
  gws docs +page-setup --document DOC_ID --page-color '#f0f0f0'
  gws docs +page-setup --document DOC_ID --mode pageless

PAPER SIZES:
  letter (8.5\" x 11\"), tabloid (11\" x 17\"), legal (8.5\" x 14\"),
  statement (5.5\" x 8.5\"), executive (7.25\" x 10.5\"), folio (8.5\" x 13\"),
  a3 (11.69\" x 16.54\"), a4 (8.27\" x 11.69\"), a5 (5.83\" x 8.27\"),
  b4 (9.84\" x 13.90\"), b5 (6.93\" x 9.84\")",
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
            let sub_matches = if let Some(m) = matches.subcommand_matches("+write") {
                let (params_str, body_str, _) = build_write_request(m, doc)?;
                Some((m, params_str, body_str))
            } else if let Some(m) = matches.subcommand_matches("+page-setup") {
                let (params_str, body_str, _) = build_page_setup_request(m, doc)?;
                Some((m, params_str, body_str))
            } else {
                None
            };

            if let Some((sub_matches, params_str, body_str)) = sub_matches {
                let documents_res = doc.resources.get("documents").ok_or_else(|| {
                    GwsError::Discovery("Resource 'documents' not found".to_string())
                })?;
                let batch_update_method =
                    documents_res.methods.get("batchUpdate").ok_or_else(|| {
                        GwsError::Discovery(
                            "Method 'documents.batchUpdate' not found".to_string(),
                        )
                    })?;

                let scope_strs: Vec<&str> = batch_update_method
                    .scopes
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                let (token, auth_method) = match auth::get_token(&scope_strs).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) if sub_matches.get_flag("dry-run") => {
                        (None, executor::AuthMethod::None)
                    }
                    Err(e) => return Err(GwsError::Auth(format!("Docs auth failed: {e}"))),
                };

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
                    sub_matches.get_flag("dry-run"),
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

fn batch_update_scopes(
    doc: &crate::discovery::RestDescription,
) -> Result<Vec<String>, GwsError> {
    let documents_res = doc
        .resources
        .get("documents")
        .ok_or_else(|| GwsError::Discovery("Resource 'documents' not found".to_string()))?;
    let method = documents_res.methods.get("batchUpdate").ok_or_else(|| {
        GwsError::Discovery("Method 'documents.batchUpdate' not found".to_string())
    })?;
    Ok(method.scopes.iter().map(|s| s.to_string()).collect())
}

fn build_write_request(
    matches: &ArgMatches,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let document_id = matches.get_one::<String>("document").unwrap();
    let text = matches.get_one::<String>("text").unwrap();

    let params = json!({ "documentId": document_id });
    let body = json!({
        "requests": [{
            "insertText": {
                "text": text,
                "endOfSegmentLocation": { "segmentId": "" }
            }
        }]
    });

    Ok((params.to_string(), body.to_string(), batch_update_scopes(doc)?))
}

fn build_page_setup_request(
    matches: &ArgMatches,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let document_id = matches.get_one::<String>("document").unwrap();

    let mode = matches.get_one::<String>("mode");
    let orientation = matches.get_one::<String>("orientation");
    let paper_size = matches.get_one::<String>("paper-size");
    let margin_top = matches.get_one::<f64>("margin-top");
    let margin_bottom = matches.get_one::<f64>("margin-bottom");
    let margin_left = matches.get_one::<f64>("margin-left");
    let margin_right = matches.get_one::<f64>("margin-right");
    let page_color = matches.get_one::<String>("page-color");

    // At least one property must be specified.
    if mode.is_none()
        && orientation.is_none()
        && paper_size.is_none()
        && margin_top.is_none()
        && margin_bottom.is_none()
        && margin_left.is_none()
        && margin_right.is_none()
        && page_color.is_none()
    {
        return Err(GwsError::Validation(
            "At least one page-setup property must be specified \
             (--mode, --orientation, --paper-size, --margin-*, --page-color)"
                .to_string(),
        ));
    }

    let mut style = serde_json::Map::new();
    let mut fields: Vec<&str> = Vec::new();

    // Document mode (pages vs pageless).
    if let Some(mode_str) = mode {
        let api_mode = match mode_str.as_str() {
            "pages" => "PAGES",
            "pageless" => "PAGELESS",
            _ => unreachable!(), // clap validates
        };
        style.insert(
            "documentFormat".to_string(),
            json!({ "documentMode": api_mode }),
        );
        fields.push("documentFormat.documentMode");
    }

    // Paper size: always stored as portrait dimensions; orientation handled via flipPageOrientation.
    if let Some(size_name) = paper_size {
        let (w, h) = paper_size_points(size_name).unwrap(); // clap validates
        style.insert(
            "pageSize".to_string(),
            json!({
                "width":  { "magnitude": w, "unit": "PT" },
                "height": { "magnitude": h, "unit": "PT" },
            }),
        );
        fields.push("pageSize");
    }

    // Orientation: portrait = no flip, landscape = flip.
    if let Some(orient) = orientation {
        let flip = orient == "landscape";
        style.insert("flipPageOrientation".to_string(), json!(flip));
        fields.push("flipPageOrientation");
    }

    // Margins (inches → points).
    if let Some(&val) = margin_top {
        style.insert(
            "marginTop".to_string(),
            json!({ "magnitude": val * POINTS_PER_INCH, "unit": "PT" }),
        );
        fields.push("marginTop");
    }
    if let Some(&val) = margin_bottom {
        style.insert(
            "marginBottom".to_string(),
            json!({ "magnitude": val * POINTS_PER_INCH, "unit": "PT" }),
        );
        fields.push("marginBottom");
    }
    if let Some(&val) = margin_left {
        style.insert(
            "marginLeft".to_string(),
            json!({ "magnitude": val * POINTS_PER_INCH, "unit": "PT" }),
        );
        fields.push("marginLeft");
    }
    if let Some(&val) = margin_right {
        style.insert(
            "marginRight".to_string(),
            json!({ "magnitude": val * POINTS_PER_INCH, "unit": "PT" }),
        );
        fields.push("marginRight");
    }

    // Page color.
    if let Some(hex) = page_color {
        let (r, g, b) = parse_hex_color(hex)?;
        style.insert(
            "background".to_string(),
            json!({
                "color": {
                    "color": {
                        "rgbColor": { "red": r, "green": g, "blue": b }
                    }
                }
            }),
        );
        fields.push("background.color");
    }

    let params = json!({ "documentId": document_id });
    let body = json!({
        "requests": [{
            "updateDocumentStyle": {
                "documentStyle": Value::Object(style),
                "fields": fields.join(","),
            }
        }]
    });

    Ok((params.to_string(), body.to_string(), batch_update_scopes(doc)?))
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

    fn make_matches_page_setup(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("document").long("document"))
            .arg(
                Arg::new("mode")
                    .long("mode")
                    .value_parser(["pages", "pageless"]),
            )
            .arg(
                Arg::new("orientation")
                    .long("orientation")
                    .value_parser(["portrait", "landscape"]),
            )
            .arg(
                Arg::new("paper-size")
                    .long("paper-size")
                    .value_parser([
                        "letter", "tabloid", "legal", "statement", "executive", "folio", "a3",
                        "a4", "a5", "b4", "b5",
                    ]),
            )
            .arg(
                Arg::new("margin-top")
                    .long("margin-top")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("margin-bottom")
                    .long("margin-bottom")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("margin-left")
                    .long("margin-left")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(
                Arg::new("margin-right")
                    .long("margin-right")
                    .value_parser(clap::value_parser!(f64)),
            )
            .arg(Arg::new("page-color").long("page-color"));
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
    fn test_paper_size_points_known() {
        let (w, h) = paper_size_points("letter").unwrap();
        assert!((w - 612.0).abs() < 0.01);
        assert!((h - 792.0).abs() < 0.01);

        let (w, h) = paper_size_points("a4").unwrap();
        assert!((w - 595.44).abs() < 0.01);
        assert!((h - 841.68).abs() < 0.01);
    }

    #[test]
    fn test_paper_size_points_unknown() {
        assert!(paper_size_points("unknown").is_none());
    }

    #[test]
    fn test_parse_hex_color_with_hash() {
        let (r, g, b) = parse_hex_color("#ff8000").unwrap();
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 0.502).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_hex_color_without_hash() {
        let (r, g, b) = parse_hex_color("000000").unwrap();
        assert!((r - 0.0).abs() < 0.01);
        assert!((g - 0.0).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_hex_color_invalid_length() {
        assert!(parse_hex_color("fff").is_err());
    }

    #[test]
    fn test_parse_hex_color_invalid_chars() {
        assert!(parse_hex_color("gggggg").is_err());
    }

    #[test]
    fn test_page_setup_paper_size_only() {
        let doc = make_mock_doc();
        let matches = make_matches_page_setup(&["test", "--document", "doc1", "--paper-size", "a4"]);
        let (params, body, _) = build_page_setup_request(&matches, &doc).unwrap();

        assert!(params.contains("doc1"));
        let body: Value = serde_json::from_str(&body).unwrap();
        let req = &body["requests"][0]["updateDocumentStyle"];
        let page_size = &req["documentStyle"]["pageSize"];
        assert!((page_size["width"]["magnitude"].as_f64().unwrap() - 595.44).abs() < 0.01);
        assert!((page_size["height"]["magnitude"].as_f64().unwrap() - 841.68).abs() < 0.01);
        assert_eq!(req["fields"], "pageSize");
    }

    #[test]
    fn test_page_setup_orientation_landscape() {
        let doc = make_mock_doc();
        let matches = make_matches_page_setup(&[
            "test",
            "--document",
            "doc1",
            "--orientation",
            "landscape",
        ]);
        let (_, body, _) = build_page_setup_request(&matches, &doc).unwrap();

        let body: Value = serde_json::from_str(&body).unwrap();
        let req = &body["requests"][0]["updateDocumentStyle"];
        assert_eq!(req["documentStyle"]["flipPageOrientation"], true);
        assert_eq!(req["fields"], "flipPageOrientation");
    }

    #[test]
    fn test_page_setup_orientation_portrait() {
        let doc = make_mock_doc();
        let matches = make_matches_page_setup(&[
            "test",
            "--document",
            "doc1",
            "--orientation",
            "portrait",
        ]);
        let (_, body, _) = build_page_setup_request(&matches, &doc).unwrap();

        let body: Value = serde_json::from_str(&body).unwrap();
        let req = &body["requests"][0]["updateDocumentStyle"];
        assert_eq!(req["documentStyle"]["flipPageOrientation"], false);
    }

    #[test]
    fn test_page_setup_margins() {
        let doc = make_mock_doc();
        let matches = make_matches_page_setup(&[
            "test",
            "--document",
            "doc1",
            "--margin-top",
            "1.5",
            "--margin-left",
            "0.75",
        ]);
        let (_, body, _) = build_page_setup_request(&matches, &doc).unwrap();

        let body: Value = serde_json::from_str(&body).unwrap();
        let style = &body["requests"][0]["updateDocumentStyle"]["documentStyle"];
        assert!((style["marginTop"]["magnitude"].as_f64().unwrap() - 108.0).abs() < 0.01);
        assert!((style["marginLeft"]["magnitude"].as_f64().unwrap() - 54.0).abs() < 0.01);
        assert!(style["marginBottom"].is_null());
        assert!(style["marginRight"].is_null());
    }

    #[test]
    fn test_page_setup_page_color() {
        let doc = make_mock_doc();
        let matches =
            make_matches_page_setup(&["test", "--document", "doc1", "--page-color", "#ff0000"]);
        let (_, body, _) = build_page_setup_request(&matches, &doc).unwrap();

        let body: Value = serde_json::from_str(&body).unwrap();
        let rgb = &body["requests"][0]["updateDocumentStyle"]["documentStyle"]["background"]
            ["color"]["color"]["rgbColor"];
        assert!((rgb["red"].as_f64().unwrap() - 1.0).abs() < 0.01);
        assert!((rgb["green"].as_f64().unwrap() - 0.0).abs() < 0.01);
        assert!((rgb["blue"].as_f64().unwrap() - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_page_setup_combined_fields_mask() {
        let doc = make_mock_doc();
        let matches = make_matches_page_setup(&[
            "test",
            "--document",
            "doc1",
            "--paper-size",
            "letter",
            "--orientation",
            "landscape",
            "--margin-top",
            "1",
        ]);
        let (_, body, _) = build_page_setup_request(&matches, &doc).unwrap();

        let body: Value = serde_json::from_str(&body).unwrap();
        let fields = body["requests"][0]["updateDocumentStyle"]["fields"]
            .as_str()
            .unwrap();
        assert!(fields.contains("pageSize"));
        assert!(fields.contains("flipPageOrientation"));
        assert!(fields.contains("marginTop"));
    }

    #[test]
    fn test_page_setup_mode_pageless() {
        let doc = make_mock_doc();
        let matches =
            make_matches_page_setup(&["test", "--document", "doc1", "--mode", "pageless"]);
        let (_, body, _) = build_page_setup_request(&matches, &doc).unwrap();

        let body: Value = serde_json::from_str(&body).unwrap();
        let req = &body["requests"][0]["updateDocumentStyle"];
        assert_eq!(
            req["documentStyle"]["documentFormat"]["documentMode"],
            "PAGELESS"
        );
        assert_eq!(req["fields"], "documentFormat.documentMode");
    }

    #[test]
    fn test_page_setup_mode_pages() {
        let doc = make_mock_doc();
        let matches = make_matches_page_setup(&["test", "--document", "doc1", "--mode", "pages"]);
        let (_, body, _) = build_page_setup_request(&matches, &doc).unwrap();

        let body: Value = serde_json::from_str(&body).unwrap();
        let req = &body["requests"][0]["updateDocumentStyle"];
        assert_eq!(
            req["documentStyle"]["documentFormat"]["documentMode"],
            "PAGES"
        );
    }

    #[test]
    fn test_page_setup_no_properties_returns_error() {
        let doc = make_mock_doc();
        let matches = make_matches_page_setup(&["test", "--document", "doc1"]);
        let result = build_page_setup_request(&matches, &doc);
        assert!(result.is_err());
    }

    #[test]
    fn test_page_setup_inject_commands() {
        let helper = DocsHelper;
        let doc = make_mock_doc();
        let cmd = Command::new("docs");
        let cmd = helper.inject_commands(cmd, &doc);
        let sub = cmd.find_subcommand("+page-setup");
        assert!(sub.is_some(), "+page-setup subcommand must be registered");
    }
}

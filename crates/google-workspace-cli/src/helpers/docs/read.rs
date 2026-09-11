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

//! Translate Docs structure, rather than render its visual layout. Keep API
//! indices and metadata; never calculate edit offsets from extracted text.

use crate::discovery::RestDescription;
use crate::error::GwsError;
use crate::executor::{self, AuthMethod, PaginationConfig};
use crate::formatter::{format_value, OutputFormat};
use crate::helpers::modelarmor::SanitizeConfig;
use clap::{Arg, ArgMatches, Command};
use serde_json::{json, Map, Value};
use std::future::Future;

pub(super) fn command() -> Command {
    Command::new("+read")
        .about("[Helper] Read a document as compact structured content")
        .arg(Arg::new("document").long("document").help("Document ID").required(true).value_name("ID"))
        .arg(Arg::new("params").long("params").help("Additional documents.get API parameters as JSON").value_name("JSON"))
        .after_help(
            "\
EXAMPLES:
  gws docs +read --document DOC_ID
  gws docs +read --document DOC_ID --format yaml
  gws docs +read --document DOC_ID --params '{\"fields\":\"*\"}' --dry-run
  gws docs +read --document DOC_ID | jq '.outline'
  gws docs +read --document DOC_ID | jq '.. | objects | select(.paragraphStyle?.headingId? == \"HEADING_ID\")'

TIPS:
  Requests all tabs with includeTabsContent=true and suggestionsViewMode=SUGGESTIONS_INLINE.
  Only those tab/suggestion options are supported; fields must be absent or exactly \"*\".
  Other documents.get options pass through --params; alt must be json. $fields is rejected.
  JSON/YAML preserve the structured view; table/CSV use the global formatter's array summary.
  tabs[].blocks and childTabs keep API order; outline lists headings with tab IDs and JSON Pointer paths.
  Paragraph text concatenates text runs only. elements retain styles, links, suggestion IDs and reference markers.
  Tables contain rows[].cells[].blocks recursively. Headers, footers and footnotes have separate blocks.
  figures contain image/drawing metadata, including alt text and URIs when returned; images are never downloaded.
  Unknown blocks, inline elements and tab types retain type=unknown markers and raw data.
  startIndex/endIndex are the API's UTF-16 offsets, scoped to each tab/segment; never offsets into extracted text.
  revisionId and suggestionsViewMode are retained when returned. Missing revisionId is not synthesized.
  source=legacyBody indicates a fallback response without populated tabs; all-tab coverage cannot be confirmed.
  This is a content view, not a layout renderer or lossless API round trip. Inherited styles are not resolved.
  Suggestions remain inline, including proposed deletions; this helper does not accept or reject suggestions.
  Use raw documents get for unsupported views or field masks. Missing body content produces an error.
  --dry-run validates and prints a request plan without acquiring credentials or fetching document content.
  --sanitize uses the existing Model Armor policy before normalization and retains _sanitization metadata.",
        )
}

pub(super) async fn handle(
    doc: &RestDescription,
    matches: &ArgMatches,
    sanitize: &SanitizeConfig,
) -> Result<(), GwsError> {
    let output = run(doc, matches, sanitize, async {
        crate::auth::get_token(&["https://www.googleapis.com/auth/documents.readonly"])
            .await
            .map_err(|e| GwsError::Auth(format!("Docs auth failed: {e}")))
    })
    .await?;
    println!("{output}");
    Ok(())
}

/// Token acquisition is lazy so local validation and dry-run never read
/// credentials. The executor remains responsible for HTTP and sanitization.
pub(super) async fn run(
    doc: &RestDescription,
    matches: &ArgMatches,
    sanitize: &SanitizeConfig,
    token: impl Future<Output = Result<String, GwsError>>,
) -> Result<String, GwsError> {
    let params = build_params(
        matches.get_one::<String>("document").unwrap(),
        matches.get_one::<String>("params").map(String::as_str),
    )?;
    let method = doc
        .resources
        .get("documents")
        .and_then(|r| r.methods.get("get"))
        .ok_or_else(|| GwsError::Discovery("Method 'documents.get' not found".into()))?;
    let dry_run = matches.get_flag("dry-run");
    let token = if dry_run { None } else { Some(token.await?) };
    let format = matches
        .get_one::<String>("format")
        .map(|f| OutputFormat::from_str(f))
        .unwrap_or_default();
    let result = executor::execute_method(
        doc,
        method,
        Some(&params.to_string()),
        None,
        token.as_deref(),
        if token.is_some() {
            AuthMethod::OAuth
        } else {
            AuthMethod::None
        },
        None,
        None,
        dry_run,
        &PaginationConfig::default(),
        sanitize.template.as_deref(),
        &sanitize.mode,
        &format,
        true,
    )
    .await?
    .ok_or_else(|| invalid_content("expected a JSON document response"))?;
    let output = if dry_run { result } else { normalize(&result)? };
    Ok(format_value(&output, &format))
}

pub(super) fn build_params(document: &str, params: Option<&str>) -> Result<Value, GwsError> {
    crate::validate::validate_resource_name(document)?;
    let mut params: Map<String, Value> = match params {
        Some(raw) => serde_json::from_str(raw)
            .map_err(|e| GwsError::Validation(format!("Invalid --params JSON object: {e}")))?,
        None => Map::new(),
    };
    // A complete response is required for normalization. A narrow allowlist is
    // deliberate: parsing arbitrary nested masks cannot prove completeness as
    // the Docs API grows. Reject the system-parameter alias as well.
    if params.contains_key("$fields") || params.get("fields").is_some_and(|v| v != "*") {
        return Err(GwsError::Validation(
            "docs +read requires all content: omit fields or use \"*\"; $fields is unsupported"
                .into(),
        ));
    }
    for (key, required) in [
        ("documentId", json!(document)),
        ("includeTabsContent", json!(true)),
        ("suggestionsViewMode", json!("SUGGESTIONS_INLINE")),
    ] {
        if params.get(key).is_some_and(|v| v != &required) {
            return Err(GwsError::Validation(format!(
                "docs +read requires {key}={required}"
            )));
        }
        params.insert(key.into(), required);
    }
    if params.get("alt").is_some_and(|v| v != "json") {
        return Err(GwsError::Validation("docs +read requires alt=json".into()));
    }
    Ok(Value::Object(params))
}

fn invalid_content(detail: &str) -> GwsError {
    GwsError::Validation(format!(
        "Incomplete or invalid Docs response: {detail}; use raw documents get to inspect it"
    ))
}

fn object(value: &Value) -> Result<Map<String, Value>, GwsError> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| invalid_content("expected an object"))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], GwsError> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| invalid_content(&format!("missing or invalid {field} array")))
}

const TAB_CONTENT: &[&str] = &[
    "body",
    "headers",
    "footers",
    "footnotes",
    "inlineObjects",
    "positionedObjects",
    "lists",
    "namedStyles",
    "namedRanges",
    "suggestedNamedStylesChanges",
];

pub(super) fn normalize(document: &Value) -> Result<Value, GwsError> {
    let mut result = object(document)?;
    let mut outline = Vec::new();
    let tabs = match document.get("tabs") {
        Some(_) => array(document, "tabs")?,
        None => &[],
    };
    let (source, tabs) = if tabs.is_empty() {
        let mut tab = Map::from_iter([
            ("tabId".into(), Value::Null),
            ("parentTabId".into(), Value::Null),
            ("childTabs".into(), json!([])),
        ]);
        if let Some(title) = document.get("title") {
            tab.insert("title".into(), title.clone());
        }
        let content: Map<String, Value> = TAB_CONTENT
            .iter()
            .filter_map(|key| document.get(key).map(|v| ((*key).into(), v.clone())))
            .collect();
        tab.extend(contents(
            &Value::Object(content),
            &Value::Null,
            "/tabs/0",
            &mut outline,
        )?);
        ("legacyBody", vec![Value::Object(tab)])
    } else {
        let tabs = tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| normalize_tab(tab, &Value::Null, &format!("/tabs/{i}"), &mut outline))
            .collect::<Result<Vec<_>, _>>()?;
        ("tabs", tabs)
    };
    for field in TAB_CONTENT {
        result.remove(*field);
    }
    result.insert("source".into(), json!(source));
    result.insert("tabs".into(), json!(tabs));
    result.insert("outline".into(), json!(outline));
    Ok(Value::Object(result))
}

fn normalize_tab(
    tab: &Value,
    parent: &Value,
    path: &str,
    outline: &mut Vec<Value>,
) -> Result<Value, GwsError> {
    let mut result = tab
        .get("tabProperties")
        .map(object)
        .transpose()?
        .unwrap_or_default();
    result
        .entry("parentTabId")
        .or_insert_with(|| parent.clone());
    let id = result.get("tabId").cloned().unwrap_or(Value::Null);
    if let Some(content) = tab.get("documentTab") {
        result.extend(contents(content, &id, path, outline)?);
        let mut extra = object(tab)?;
        for key in ["tabProperties", "documentTab", "childTabs"] {
            extra.remove(key);
        }
        if !extra.is_empty() {
            result.insert("metadata".into(), Value::Object(extra));
        }
    } else {
        result.insert("type".into(), json!("unknown"));
        result.insert("data".into(), tab.clone());
    }
    let children = if tab.get("childTabs").is_some() {
        array(tab, "childTabs")?
    } else {
        &[]
    };
    let children = children
        .iter()
        .enumerate()
        .map(|(i, tab)| normalize_tab(tab, &id, &format!("{path}/childTabs/{i}"), outline))
        .collect::<Result<Vec<_>, _>>()?;
    result.insert("childTabs".into(), json!(children));
    Ok(Value::Object(result))
}

fn contents(
    content: &Value,
    tab_id: &Value,
    path: &str,
    outline: &mut Vec<Value>,
) -> Result<Map<String, Value>, GwsError> {
    let mut result = object(content)?;
    let body = result
        .remove("body")
        .ok_or_else(|| invalid_content("missing body"))?;
    result.insert(
        "blocks".into(),
        blocks(
            array(&body, "content")?,
            tab_id,
            &format!("{path}/blocks"),
            outline,
        )?,
    );
    let mut body_metadata = object(&body)?;
    body_metadata.remove("content");
    if !body_metadata.is_empty() {
        result.insert("bodyMetadata".into(), Value::Object(body_metadata));
    }
    for kind in ["headers", "footers", "footnotes"] {
        if let Some(segments) = result.get_mut(kind) {
            let mut normalized = Map::new();
            for (id, segment) in object(segments)? {
                let mut segment_result = object(&segment)?;
                // JSON Pointer escaping, not URL escaping.
                let pointer_id = id.replace('~', "~0").replace('/', "~1");
                segment_result.insert(
                    "blocks".into(),
                    blocks(
                        array(&segment, "content")?,
                        tab_id,
                        &format!("{path}/{kind}/{pointer_id}/blocks"),
                        outline,
                    )?,
                );
                segment_result.remove("content");
                normalized.insert(id, Value::Object(segment_result));
            }
            *segments = Value::Object(normalized);
        }
    }
    let mut figures = Map::new();
    for (field, properties, placement) in [
        ("inlineObjects", "inlineObjectProperties", "inline"),
        (
            "positionedObjects",
            "positionedObjectProperties",
            "positioned",
        ),
    ] {
        if let Some(objects) = result.remove(field) {
            for (id, value) in object(&objects)? {
                let mut figure = object(&value)?;
                if let Some(properties) = figure.remove(properties) {
                    figure.extend(object(&properties)?);
                }
                let kind = if figure
                    .get("embeddedObject")
                    .and_then(|v| v.get("imageProperties"))
                    .is_some()
                {
                    "image"
                } else if figure
                    .get("embeddedObject")
                    .and_then(|v| v.get("embeddedDrawingProperties"))
                    .is_some()
                {
                    "drawing"
                } else {
                    "unknown"
                };
                figure.insert("type".into(), json!(kind));
                figure.insert("placement".into(), json!(placement));
                figure.entry("objectId").or_insert_with(|| json!(id));
                figures.insert(id, Value::Object(figure));
            }
        }
    }
    if !figures.is_empty() {
        result.insert("figures".into(), Value::Object(figures));
    }
    Ok(result)
}

/// Flatten a known union arm, retaining styles, suggestions, source indices and
/// future metadata fields. Unrecognized union arms are retained as raw markers.
fn payload(value: &Value, key: &str, kind: &str) -> Result<Map<String, Value>, GwsError> {
    let mut outer = object(value)?;
    let mut inner = object(
        &outer
            .remove(key)
            .ok_or_else(|| invalid_content("missing element"))?,
    )?;
    inner.extend(outer);
    inner.insert("type".into(), json!(kind));
    Ok(inner)
}

fn unknown(value: &Value) -> Value {
    let mut marker = json!({"type": "unknown", "data": value});
    for key in ["startIndex", "endIndex"] {
        if let Some(index) = value.get(key) {
            marker[key] = index.clone();
        }
    }
    marker
}

fn element(value: &Value) -> Result<Value, GwsError> {
    for (key, kind) in [
        ("textRun", "text"),
        ("inlineObjectElement", "figure"),
        ("footnoteReference", "footnoteReference"),
        ("horizontalRule", "horizontalRule"),
        ("pageBreak", "pageBreak"),
        ("columnBreak", "columnBreak"),
        ("equation", "equation"),
        ("autoText", "autoText"),
    ] {
        if value.get(key).is_some() {
            let mut result = payload(value, key, kind)?;
            if key == "textRun" {
                let text = result
                    .remove("content")
                    .filter(Value::is_string)
                    .ok_or_else(|| invalid_content("textRun missing content"))?;
                result.insert("text".into(), text);
            } else if key == "inlineObjectElement" {
                if let Some(id) = result.remove("inlineObjectId") {
                    result.insert("objectId".into(), id);
                }
            }
            return Ok(Value::Object(result));
        }
    }
    Ok(unknown(value))
}

fn blocks(
    content: &[Value],
    tab_id: &Value,
    path: &str,
    outline: &mut Vec<Value>,
) -> Result<Value, GwsError> {
    content
        .iter()
        .enumerate()
        .map(|(i, block)| {
            let path = format!("{path}/{i}");
            if let Some(paragraph) = block.get("paragraph") {
                let mut result = payload(block, "paragraph", "paragraph")?;
                let elements = array(paragraph, "elements")?
                    .iter()
                    .map(element)
                    .collect::<Result<Vec<_>, _>>()?;
                let text: String = elements
                    .iter()
                    .filter_map(|e| e.get("text").and_then(Value::as_str))
                    .collect();
                result.insert("text".into(), json!(text));
                result.insert("elements".into(), json!(elements));
                if let Some(style) = paragraph.get("paragraphStyle") {
                    if let Some(level) =
                        style
                            .get("namedStyleType")
                            .and_then(Value::as_str)
                            .filter(|s| {
                                matches!(
                                    *s,
                                    "TITLE"
                                        | "SUBTITLE"
                                        | "HEADING_1"
                                        | "HEADING_2"
                                        | "HEADING_3"
                                        | "HEADING_4"
                                        | "HEADING_5"
                                        | "HEADING_6"
                                )
                            })
                    {
                        let mut heading =
                            json!({"tabId": tab_id, "level": level, "text": text, "path": path});
                        for key in ["startIndex", "endIndex"] {
                            if let Some(value) = block.get(key) {
                                heading[key] = value.clone();
                            }
                        }
                        if let Some(id) = style.get("headingId") {
                            heading["headingId"] = id.clone();
                        }
                        outline.push(heading);
                    }
                }
                Ok(Value::Object(result))
            } else if let Some(table) = block.get("table") {
                let mut result = payload(block, "table", "table")?;
                let rows = array(table, "tableRows")?
                    .iter()
                    .enumerate()
                    .map(|(r, row)| {
                        let mut normalized = object(row)?;
                        let cells = array(row, "tableCells")?
                            .iter()
                            .enumerate()
                            .map(|(c, cell)| {
                                let mut normalized = object(cell)?;
                                normalized.insert(
                                    "blocks".into(),
                                    blocks(
                                        array(cell, "content")?,
                                        tab_id,
                                        &format!("{path}/rows/{r}/cells/{c}/blocks"),
                                        outline,
                                    )?,
                                );
                                normalized.remove("content");
                                Ok(Value::Object(normalized))
                            })
                            .collect::<Result<Vec<_>, GwsError>>()?;
                        normalized.remove("tableCells");
                        normalized.insert("cells".into(), json!(cells));
                        Ok(Value::Object(normalized))
                    })
                    .collect::<Result<Vec<_>, GwsError>>()?;
                if let Some(count) = result.remove("rows") {
                    result.insert("rowCount".into(), count);
                }
                result.remove("tableRows");
                result.insert("rows".into(), json!(rows));
                Ok(Value::Object(result))
            } else if let Some(toc) = block.get("tableOfContents") {
                let mut result = payload(block, "tableOfContents", "tableOfContents")?;
                result.insert(
                    "blocks".into(),
                    blocks(
                        array(toc, "content")?,
                        tab_id,
                        &format!("{path}/blocks"),
                        outline,
                    )?,
                );
                result.remove("content");
                Ok(Value::Object(result))
            } else if block.get("sectionBreak").is_some() {
                payload(block, "sectionBreak", "sectionBreak").map(Value::Object)
            } else {
                Ok(unknown(block))
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

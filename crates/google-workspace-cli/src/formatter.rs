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

//! Output Formatting
//!
//! Transforms JSON API responses into human-readable formats (table, YAML, CSV).

use serde_json::Value;
use std::fmt::Write;

/// Supported output formats.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum OutputFormat {
    /// Pretty-printed JSON (default).
    #[default]
    Json,
    /// Aligned text table.
    Table,
    /// YAML.
    Yaml,
    /// Comma-separated values.
    Csv,
}

impl OutputFormat {
    /// Parse from a string argument.
    ///
    /// Returns `Ok(format)` for known values, or `Err(unknown_value)` if the
    /// string is not recognised.  Call sites should warn the user on `Err` and
    /// decide whether to fall back to JSON or surface an error.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "table" => Ok(Self::Table),
            "yaml" | "yml" => Ok(Self::Yaml),
            "csv" => Ok(Self::Csv),
            other => Err(other.to_string()),
        }
    }

    /// Parse from a string argument, falling back to JSON for unknown values.
    ///
    /// Prefer `parse()` at call sites where you want to surface a warning.
    pub fn from_str(s: &str) -> Self {
        Self::parse(s).unwrap_or(Self::Json)
    }
}

/// Format a JSON value according to the specified output format.
pub fn format_value(value: &Value, format: &OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(value).unwrap_or_default(),
        OutputFormat::Table => format_table(value),
        OutputFormat::Yaml => format_yaml(value),
        OutputFormat::Csv => format_csv(value),
    }
}

/// Format a JSON value for a paginated page.
///
/// When auto-paginating with `--page-all`, CSV and table formats should only
/// emit column headers on the **first** page so that each subsequent page
/// contains only data rows, making the combined output machine-parseable.
///
/// For JSON the output is compact (one JSON object per line / NDJSON).
/// For YAML each page is prefixed with a `---` document separator so the
/// combined stream is a valid YAML multi-document file.
pub fn format_value_paginated(value: &Value, format: &OutputFormat, is_first_page: bool) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string(value).unwrap_or_default(),
        OutputFormat::Csv => format_csv_page(value, is_first_page),
        OutputFormat::Table => format_table_page(value, is_first_page),
        // Prefix every page with a YAML document separator so that the
        // concatenated stream is parseable as a multi-document YAML file.
        OutputFormat::Yaml => format!("---\n{}", format_yaml(value)),
    }
}

/// Extract a "data array" from a typical Google API list response.
/// Google APIs return lists as `{ "files": [...], "nextPageToken": "..." }`
/// where the array key varies by resource type.
fn extract_items(value: &Value) -> Option<(&str, &Vec<Value>)> {
    if let Value::Object(obj) = value {
        for (key, val) in obj {
            if key == "nextPageToken" || key == "kind" || key.starts_with('_') {
                continue;
            }
            if let Value::Array(arr) = val {
                if !arr.is_empty() {
                    return Some((key, arr));
                }
            }
        }
    }
    None
}

fn format_table(value: &Value) -> String {
    format_table_page(value, true)
}

/// Recursively flatten a JSON object into `(dot.notation.key, string_value)` pairs.
///
/// Nested objects become `parent.child` key names so that `--format table` can
/// render them as individual columns instead of raw JSON blobs.
fn flatten_object(obj: &serde_json::Map<String, Value>, prefix: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (key, val) in obj {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match val {
            Value::Object(nested) => {
                out.extend(flatten_object(nested, &full_key));
            }
            _ => {
                out.push((full_key, value_to_cell(val)));
            }
        }
    }
    out
}

/// Format as a text table, optionally omitting the header row.
///
/// Pass `emit_header = false` for continuation pages when using `--page-all`
/// so the combined terminal output doesn't repeat column names and separator
/// lines between pages.
fn format_table_page(value: &Value, emit_header: bool) -> String {
    // Try to extract a list of items from standard Google API response
    let items = extract_items(value);

    if let Some((_key, arr)) = items {
        format_array_as_table(arr, emit_header)
    } else if let Value::Array(arr) = value {
        format_array_as_table(arr, emit_header)
    } else if let Value::Object(obj) = value {
        // Single object: key/value table — flatten nested objects first
        let mut output = String::new();
        let flat = flatten_object(obj, "");
        let max_key_len = flat.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (key, val_str) in &flat {
            let _ = writeln!(output, "{:width$}  {}", key, val_str, width = max_key_len);
        }
        output
    } else {
        value.to_string()
    }
}

fn format_array_as_table(arr: &[Value], emit_header: bool) -> String {
    if arr.is_empty() {
        return "(empty)\n".to_string();
    }

    // Flatten each row so nested objects become dot-notation columns.
    let flat_rows: Vec<Vec<(String, String)>> = arr
        .iter()
        .map(|item| match item {
            Value::Object(obj) => flatten_object(obj, ""),
            _ => vec![(String::new(), value_to_cell(item))],
        })
        .collect();

    // Collect all unique column names (preserving insertion order).
    let mut columns: Vec<String> = Vec::new();
    for row in &flat_rows {
        for (key, _) in row {
            if !columns.contains(key) {
                columns.push(key.clone());
            }
        }
    }

    if columns.is_empty() {
        // Array of non-objects
        let mut output = String::new();
        for item in arr {
            let _ = writeln!(output, "{}", value_to_cell(item));
        }
        return output;
    }

    // Build lookup: row_index -> column_name -> cell_value
    let row_maps: Vec<std::collections::HashMap<&str, &str>> = flat_rows
        .iter()
        .map(|pairs| {
            pairs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect()
        })
        .collect();

    // Calculate column widths (char-count, not byte-count).
    let mut widths: Vec<usize> = columns.iter().map(|c| c.chars().count()).collect();
    let rows: Vec<Vec<String>> = row_maps
        .iter()
        .map(|row| {
            columns
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    let cell = row.get(col.as_str()).copied().unwrap_or("").to_string();
                    let char_len = cell.chars().count();
                    if char_len > widths[i] {
                        widths[i] = char_len;
                    }
                    // Cap column width at 60 chars
                    if widths[i] > 60 {
                        widths[i] = 60;
                    }
                    cell
                })
                .collect()
        })
        .collect();

    let mut output = String::new();

    if emit_header {
        // Header
        let header: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:width$}", c, width = widths[i]))
            .collect();
        let _ = writeln!(output, "{}", header.join("  "));

        // Separator
        let sep: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
        let _ = writeln!(output, "{}", sep.join("  "));
    }

    // Rows — truncate by char count to avoid panicking on multi-byte UTF-8.
    for row in &rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let char_len = c.chars().count();
                let truncated = if char_len > widths[i] {
                    // Safe char-boundary slice: take widths[i]-1 chars, then append ellipsis.
                    let truncated_str: String = c.chars().take(widths[i] - 1).collect();
                    format!("{truncated_str}…")
                } else {
                    c.clone()
                };
                // Pad to column width (by char count)
                let pad = widths[i].saturating_sub(truncated.chars().count());
                format!("{truncated}{}", " ".repeat(pad))
            })
            .collect();
        let _ = writeln!(output, "{}", cells.join("  "));
    }

    output
}

fn format_yaml(value: &Value) -> String {
    json_to_yaml(value, 0)
}

fn json_to_yaml(value: &Value, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.contains('\n') {
                // Genuine multi-line content: block scalar is the most readable choice.
                format!(
                    "|\n{}",
                    s.lines()
                        .map(|l| format!("{prefix}  {l}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            } else {
                // Single-line strings: always double-quote so that characters like
                // `#` (comment marker) and `:` (mapping indicator) are never
                // misinterpreted by YAML parsers.  Escape backslashes and double
                // quotes to keep the output valid.
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                return "[]".to_string();
            }
            let mut out = String::new();
            for item in arr {
                let val_str = json_to_yaml(item, indent + 1);
                let _ = write!(out, "\n{prefix}- {val_str}");
            }
            out
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                return "{}".to_string();
            }
            let mut out = String::new();
            for (key, val) in obj {
                match val {
                    Value::Object(_) | Value::Array(_) => {
                        let val_str = json_to_yaml(val, indent + 1);
                        let _ = write!(out, "\n{prefix}{key}:{val_str}");
                    }
                    _ => {
                        let val_str = json_to_yaml(val, indent);
                        let _ = write!(out, "\n{prefix}{key}: {val_str}");
                    }
                }
            }
            out
        }
    }
}

fn format_csv(value: &Value) -> String {
    format_csv_page(value, true)
}

/// Format as CSV, optionally omitting the header row.
///
/// Pass `emit_header = false` for all pages after the first when using
/// `--page-all`, so the combined output has a single header line.
fn format_csv_page(value: &Value, emit_header: bool) -> String {
    let items = extract_items(value);

    let arr = if let Some((_key, arr)) = items {
        arr.as_slice()
    } else if let Value::Array(arr) = value {
        arr.as_slice()
    } else {
        // Single value — just output it
        return value_to_cell(value);
    };

    if arr.is_empty() {
        return String::new();
    }

    // Array of non-objects
    if !arr.iter().any(|v| v.is_object()) {
        let mut output = String::new();
        for item in arr {
            if let Value::Array(inner) = item {
                let cells: Vec<String> = inner
                    .iter()
                    .map(|v| csv_escape(&value_to_cell(v)))
                    .collect();
                let _ = writeln!(output, "{}", cells.join(","));
            } else {
                let _ = writeln!(output, "{}", csv_escape(&value_to_cell(item)));
            }
        }
        return output;
    }

    // Collect columns
    let mut columns: Vec<String> = Vec::new();
    for item in arr {
        if let Value::Object(obj) = item {
            for key in obj.keys() {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
            }
        }
    }

    let mut output = String::new();

    // Header (omitted on continuation pages)
    if emit_header {
        let _ = writeln!(output, "{}", columns.join(","));
    }

    // Rows
    for item in arr {
        let cells: Vec<String> = columns
            .iter()
            .map(|col| {
                if let Value::Object(obj) = item {
                    csv_escape(&value_to_cell(obj.get(col).unwrap_or(&Value::Null)))
                } else {
                    String::new()
                }
            })
            .collect();
        let _ = writeln!(output, "{}", cells.join(","));
    }

    output
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn value_to_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(value_to_cell).collect();
            items.join(", ")
        }
        Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// A rendered report: what goes to stdout, plus an optional notice for stderr.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedReport {
    /// Text for stdout.
    pub stdout: String,
    /// A human-facing notice (e.g. "no data in range") for stderr, used when
    /// printing it on stdout would corrupt machine-readable output.
    pub notice: Option<String>,
}

/// Render a report whose columns have a fixed, meaningful order.
///
/// `serde_json` objects sort their keys, so the generic [`format_value`]
/// cannot keep column order (a report's first column is its dimension, and a
/// form's columns follow its questions). This keeps it:
///
/// - `table`: an optional title line, then an aligned table in column order.
///   With no rows it prints `empty_note` instead of an empty table.
/// - `csv`: a header in column order, then one line per row. With no rows the
///   header is still printed (so the output stays valid CSV) and `empty_note`
///   is returned as a stderr notice.
/// - `json` / `yaml`: `meta` plus `columns`, `rows` (one object per row) and
///   `count`. With no rows a `note` field carries `empty_note`.
pub fn render_report(
    columns: &[String],
    rows: &[Vec<Value>],
    meta: &Value,
    title: Option<&str>,
    empty_note: &str,
    format: &OutputFormat,
) -> RenderedReport {
    match format {
        OutputFormat::Table => {
            let mut out = String::new();
            if let Some(t) = title {
                let _ = writeln!(out, "{t}");
            }
            if rows.is_empty() {
                let _ = writeln!(out, "{empty_note}");
            } else {
                if title.is_some() {
                    out.push('\n');
                }
                let cells: Vec<Vec<String>> = rows
                    .iter()
                    .map(|r| r.iter().map(value_to_cell).collect())
                    .collect();
                out.push_str(&ordered_table(columns, &cells));
            }
            RenderedReport {
                stdout: out,
                notice: None,
            }
        }
        OutputFormat::Csv => {
            let mut out = String::new();
            let header: Vec<String> = columns.iter().map(|c| csv_escape(c)).collect();
            let _ = writeln!(out, "{}", header.join(","));
            for row in rows {
                let cells: Vec<String> = (0..columns.len())
                    .map(|i| csv_escape(&value_to_cell(row.get(i).unwrap_or(&Value::Null))))
                    .collect();
                let _ = writeln!(out, "{}", cells.join(","));
            }
            RenderedReport {
                stdout: out,
                notice: rows.is_empty().then(|| empty_note.to_string()),
            }
        }
        OutputFormat::Json | OutputFormat::Yaml => {
            let objects: Vec<Value> = rows
                .iter()
                .map(|row| {
                    let mut obj = serde_json::Map::new();
                    for (i, col) in columns.iter().enumerate() {
                        obj.insert(col.clone(), row.get(i).cloned().unwrap_or(Value::Null));
                    }
                    Value::Object(obj)
                })
                .collect();
            let mut envelope = match meta {
                Value::Object(m) => m.clone(),
                _ => serde_json::Map::new(),
            };
            envelope.insert(
                "columns".to_string(),
                Value::Array(columns.iter().cloned().map(Value::String).collect()),
            );
            envelope.insert("count".to_string(), Value::from(objects.len()));
            envelope.insert("rows".to_string(), Value::Array(objects));
            if rows.is_empty() {
                envelope.insert("note".to_string(), Value::String(empty_note.to_string()));
            }
            let value = Value::Object(envelope);
            let mut text = if *format == OutputFormat::Json {
                serde_json::to_string_pretty(&value).unwrap_or_default()
            } else {
                format_yaml(&value)
            };
            if !text.ends_with('\n') {
                text.push('\n');
            }
            RenderedReport {
                stdout: text,
                notice: None,
            }
        }
    }
}

/// Aligned text table in the given column order (widths capped at 60 chars).
fn ordered_table(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = columns.iter().map(|c| c.chars().count()).collect();
    for row in rows {
        for (i, w) in widths.iter_mut().enumerate() {
            let len = row.get(i).map(|c| c.chars().count()).unwrap_or(0);
            *w = (*w).max(len).min(60);
        }
    }
    let fit = |cell: &str, width: usize| -> String {
        let one_line = cell.replace(['\n', '\r'], " ");
        let len = one_line.chars().count();
        let shown = if len > width {
            let head: String = one_line.chars().take(width.saturating_sub(1)).collect();
            format!("{head}…")
        } else {
            one_line
        };
        let pad = width.saturating_sub(shown.chars().count());
        format!("{shown}{}", " ".repeat(pad))
    };
    let mut out = String::new();
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| fit(c, widths[i]))
        .collect();
    let _ = writeln!(out, "{}", header.join("  ").trim_end());
    let sep: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
    let _ = writeln!(out, "{}", sep.join("  "));
    for row in rows {
        let cells: Vec<String> = (0..columns.len())
            .map(|i| fit(row.get(i).map(String::as_str).unwrap_or(""), widths[i]))
            .collect();
        let _ = writeln!(out, "{}", cells.join("  ").trim_end());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("json"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("table"), OutputFormat::Table);
        assert_eq!(OutputFormat::from_str("yaml"), OutputFormat::Yaml);
        assert_eq!(OutputFormat::from_str("yml"), OutputFormat::Yaml);
        assert_eq!(OutputFormat::from_str("csv"), OutputFormat::Csv);
        assert_eq!(OutputFormat::from_str("unknown"), OutputFormat::Json);
    }

    #[test]
    fn test_output_format_parse_known() {
        assert_eq!(OutputFormat::parse("json"), Ok(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("table"), Ok(OutputFormat::Table));
        assert_eq!(OutputFormat::parse("yaml"), Ok(OutputFormat::Yaml));
        assert_eq!(OutputFormat::parse("yml"), Ok(OutputFormat::Yaml));
        assert_eq!(OutputFormat::parse("csv"), Ok(OutputFormat::Csv));
        // Case-insensitive
        assert_eq!(OutputFormat::parse("JSON"), Ok(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("TABLE"), Ok(OutputFormat::Table));
    }

    #[test]
    fn test_output_format_parse_unknown_returns_err() {
        assert!(OutputFormat::parse("bogus").is_err());
        assert_eq!(OutputFormat::parse("bogus").unwrap_err(), "bogus");
        assert!(OutputFormat::parse("").is_err());
    }

    #[test]
    fn test_format_json() {
        let val = json!({"name": "test"});
        let output = format_value(&val, &OutputFormat::Json);
        assert!(output.contains("\"name\""));
        assert!(output.contains("\"test\""));
    }

    #[test]
    fn test_format_table_array_of_objects() {
        let val = json!({
            "files": [
                {"id": "1", "name": "hello.txt"},
                {"id": "2", "name": "world.txt"}
            ]
        });
        let output = format_value(&val, &OutputFormat::Table);
        assert!(output.contains("id"));
        assert!(output.contains("name"));
        assert!(output.contains("hello.txt"));
        assert!(output.contains("world.txt"));
        // Check separator line
        assert!(output.contains("──"));
    }

    #[test]
    fn test_format_table_single_object() {
        let val = json!({"id": "abc", "name": "test"});
        let output = format_value(&val, &OutputFormat::Table);
        assert!(output.contains("id"));
        assert!(output.contains("abc"));
    }

    #[test]
    fn test_format_table_nested_object_flattened() {
        // Nested objects should become dot-notation columns, not raw JSON blobs.
        let val = json!({
            "user": {
                "displayName": "Alice",
                "emailAddress": "alice@example.com"
            },
            "storageQuota": {
                "limit": "1000",
                "usage": "500"
            }
        });
        let output = format_value(&val, &OutputFormat::Table);
        // Should contain dot-notation keys
        assert!(
            output.contains("user.displayName"),
            "expected flattened key in output:\n{output}"
        );
        assert!(
            output.contains("user.emailAddress"),
            "expected flattened key in output:\n{output}"
        );
        assert!(
            output.contains("Alice"),
            "expected value in output:\n{output}"
        );
        // Should NOT contain raw JSON blobs
        assert!(
            !output.contains("{\"displayName"),
            "should not have raw JSON blob:\n{output}"
        );
    }

    #[test]
    fn test_format_table_nested_objects_in_array() {
        let val = json!([
            {"id": "1", "owner": {"name": "Alice"}},
            {"id": "2", "owner": {"name": "Bob"}}
        ]);
        let output = format_value(&val, &OutputFormat::Table);
        assert!(
            output.contains("owner.name"),
            "expected flattened column:\n{output}"
        );
        assert!(output.contains("Alice"), "expected value:\n{output}");
        assert!(output.contains("Bob"), "expected value:\n{output}");
    }

    #[test]
    fn test_format_table_multibyte_truncation_does_not_panic() {
        // Column width cap is 60 chars, so a long string with multi-byte chars
        // must be safely truncated without a byte-boundary panic.
        let long_emoji = "😀".repeat(70); // each emoji is 4 bytes
        let val = json!([{"col": long_emoji}]);
        // Should not panic
        let output = format_value(&val, &OutputFormat::Table);
        assert!(output.contains("col"), "column name must appear:\n{output}");
    }

    #[test]
    fn test_format_table_multibyte_exact_boundary() {
        // Multi-byte chars at various positions must not panic or produce garbled output.
        let val = json!([{"name": "café résumé naïve"}]);
        let output = format_value(&val, &OutputFormat::Table);
        assert!(output.contains("name"), "column must appear:\n{output}");
    }

    #[test]
    fn test_format_csv() {
        let val = json!({
            "files": [
                {"id": "1", "name": "hello"},
                {"id": "2", "name": "world"}
            ]
        });
        let output = format_value(&val, &OutputFormat::Csv);
        assert!(output.contains("id,name"));
        assert!(output.contains("1,hello"));
        assert!(output.contains("2,world"));
    }

    #[test]
    fn test_format_csv_array_of_arrays() {
        // Sheets API returns {"values": [["col1","col2"], ["a","b"]]}
        let val = json!({
            "values": [
                ["Student Name", "Gender", "Class Level"],
                ["Alexandra", "Female", "4. Senior"],
                ["Andrew", "Male", "1. Freshman"]
            ]
        });
        let output = format_value(&val, &OutputFormat::Csv);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "Student Name,Gender,Class Level");
        assert_eq!(lines[1], "Alexandra,Female,4. Senior");
        assert_eq!(lines[2], "Andrew,Male,1. Freshman");
    }

    #[test]
    fn test_format_csv_flat_scalars() {
        // Flat array of non-object, non-array values → one value per line
        let val = json!(["apple", "banana", "cherry"]);
        let output = format_value(&val, &OutputFormat::Csv);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "apple");
        assert_eq!(lines[1], "banana");
        assert_eq!(lines[2], "cherry");
    }

    #[test]
    fn test_format_csv_flat_scalars_with_escaping() {
        // Scalars that contain commas/quotes must be CSV-escaped
        let val = json!(["plain", "has,comma", "has\"quote"]);
        let output = format_value(&val, &OutputFormat::Csv);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "plain");
        assert_eq!(lines[1], "\"has,comma\"");
        assert_eq!(lines[2], "\"has\"\"quote\"");
    }

    #[test]
    fn test_format_csv_escape() {
        assert_eq!(csv_escape("simple"), "simple");
        assert_eq!(csv_escape("has,comma"), "\"has,comma\"");
        assert_eq!(csv_escape("has\"quote"), "\"has\"\"quote\"");
    }

    #[test]
    fn test_format_yaml() {
        let val = json!({"name": "test", "count": 42});
        let output = format_value(&val, &OutputFormat::Yaml);
        assert!(output.contains("name: \"test\""));
        assert!(output.contains("count: 42"));
    }

    #[test]
    fn test_format_table_empty_array() {
        let val = json!({"files": []});
        // No items to extract, falls back to single-object table
        let output = format_value(&val, &OutputFormat::Table);
        assert!(output.contains("files"));
    }

    #[test]
    fn test_extract_items() {
        let val = json!({"files": [{"id": "1"}], "nextPageToken": "abc"});
        let (key, items) = extract_items(&val).unwrap();
        assert_eq!(key, "files");
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_extract_items_none() {
        let val = json!({"status": "ok"});
        assert!(extract_items(&val).is_none());
    }

    // --- YAML block-scalar regression tests ---

    #[test]
    fn test_format_yaml_hash_in_string_is_quoted_not_block() {
        // `drive#file` contains `#` which is a YAML comment marker; the
        // serialiser must quote it rather than emit a block scalar.
        let val = json!({"kind": "drive#file", "id": "123"});
        let output = format_value(&val, &OutputFormat::Yaml);
        // Must be a double-quoted string, not a block scalar (`|`).
        assert!(
            output.contains("kind: \"drive#file\""),
            "expected double-quoted kind, got:\n{output}"
        );
        assert!(
            !output.contains("kind: |"),
            "kind must not use block scalar, got:\n{output}"
        );
    }

    #[test]
    fn test_format_yaml_colon_in_string_is_quoted() {
        let val = json!({"url": "https://example.com/path"});
        let output = format_value(&val, &OutputFormat::Yaml);
        assert!(
            output.contains("url: \"https://example.com/path\""),
            "expected double-quoted url, got:\n{output}"
        );
        assert!(!output.contains("url: |"), "url must not use block scalar");
    }

    #[test]
    fn test_format_yaml_multiline_still_uses_block() {
        let val = json!({"body": "line one\nline two"});
        let output = format_value(&val, &OutputFormat::Yaml);
        // Multi-line content should still use block scalar.
        assert!(
            output.contains("body: |"),
            "multiline string must use block scalar, got:\n{output}"
        );
    }

    // --- Paginated format tests ---

    #[test]
    fn test_format_value_paginated_csv_first_page_has_header() {
        let val = json!({
            "files": [
                {"id": "1", "name": "a.txt"},
                {"id": "2", "name": "b.txt"}
            ]
        });
        let output = format_value_paginated(&val, &OutputFormat::Csv, true);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "id,name", "first page must start with header");
        assert_eq!(lines[1], "1,a.txt");
    }

    #[test]
    fn test_format_value_paginated_csv_continuation_no_header() {
        let val = json!({
            "files": [
                {"id": "3", "name": "c.txt"}
            ]
        });
        let output = format_value_paginated(&val, &OutputFormat::Csv, false);
        let lines: Vec<&str> = output.lines().collect();
        // The first (and only) line must be a data row, not the header.
        assert_eq!(lines[0], "3,c.txt", "continuation page must have no header");
        assert!(
            !output.contains("id,name"),
            "header must be absent on continuation pages"
        );
    }

    #[test]
    fn test_format_value_paginated_table_first_page_has_header() {
        let val = json!({
            "items": [
                {"id": "1", "name": "foo"}
            ]
        });
        let output = format_value_paginated(&val, &OutputFormat::Table, true);
        assert!(
            output.contains("id"),
            "table header must appear on first page"
        );
        assert!(output.contains("──"), "separator must appear on first page");
    }

    #[test]
    fn test_format_value_paginated_table_continuation_no_header() {
        let val = json!({
            "items": [
                {"id": "2", "name": "bar"}
            ]
        });
        let output = format_value_paginated(&val, &OutputFormat::Table, false);
        assert!(output.contains("bar"), "data row must be present");
        assert!(
            !output.contains("──"),
            "separator must be absent on continuation pages"
        );
    }

    #[test]
    fn test_format_value_paginated_yaml_has_document_separator() {
        let val = json!({"files": [{"id": "1", "name": "foo"}]});
        let first = format_value_paginated(&val, &OutputFormat::Yaml, true);
        let second = format_value_paginated(&val, &OutputFormat::Yaml, false);
        assert!(
            first.starts_with("---\n"),
            "first YAML page must start with ---"
        );
        assert!(
            second.starts_with("---\n"),
            "continuation YAML pages must also start with ---"
        );
    }
    // ── render_report ────────────────────────────────────────────────

    fn report_cols() -> Vec<String> {
        vec!["query".to_string(), "clicks".to_string(), "ctr".to_string()]
    }

    #[test]
    fn render_report_table_keeps_column_order() {
        let rows = vec![vec![json!("mbprep"), json!(12), json!("4.0%")]];
        let r = render_report(
            &report_cols(),
            &rows,
            &json!({}),
            Some("title line"),
            "no data",
            &OutputFormat::Table,
        );
        let lines: Vec<&str> = r.stdout.lines().collect();
        assert_eq!(lines[0], "title line");
        assert!(lines[2].starts_with("query"));
        let q = lines[2].find("query").unwrap();
        let c = lines[2].find("clicks").unwrap();
        let t = lines[2].find("ctr").unwrap();
        assert!(q < c && c < t, "columns out of order: {}", lines[2]);
        assert!(lines[4].contains("mbprep") && lines[4].contains("12"));
        assert!(r.notice.is_none());
    }

    #[test]
    fn render_report_table_empty_prints_note_not_table() {
        let r = render_report(
            &report_cols(),
            &[],
            &json!({}),
            None,
            "No data in range.",
            &OutputFormat::Table,
        );
        assert_eq!(r.stdout, "No data in range.\n");
        assert!(!r.stdout.contains('─'));
    }

    #[test]
    fn render_report_table_truncates_and_flattens_newlines() {
        let long = "x".repeat(80);
        let rows = vec![vec![json!(format!("a\nb {long}")), json!(1), json!("")]];
        let r = render_report(
            &report_cols(),
            &rows,
            &json!({}),
            None,
            "",
            &OutputFormat::Table,
        );
        let data = r.stdout.lines().nth(2).unwrap();
        assert!(data.starts_with("a b xxx"));
        assert!(data.contains('…'));
    }

    #[test]
    fn render_report_csv_orders_and_escapes() {
        let rows = vec![vec![json!("a, b"), json!(3), json!("say \"hi\"")]];
        let r = render_report(
            &report_cols(),
            &rows,
            &json!({}),
            Some("ignored in csv"),
            "none",
            &OutputFormat::Csv,
        );
        assert_eq!(
            r.stdout,
            "query,clicks,ctr\n\"a, b\",3,\"say \"\"hi\"\"\"\n"
        );
        assert!(r.notice.is_none());
    }

    #[test]
    fn render_report_csv_empty_keeps_header_and_returns_notice() {
        let r = render_report(
            &report_cols(),
            &[],
            &json!({}),
            None,
            "No data in range.",
            &OutputFormat::Csv,
        );
        assert_eq!(r.stdout, "query,clicks,ctr\n");
        assert_eq!(r.notice.as_deref(), Some("No data in range."));
    }

    #[test]
    fn render_report_json_envelope() {
        let rows = vec![vec![json!("mbprep"), json!(12), json!(0.04)]];
        let r = render_report(
            &report_cols(),
            &rows,
            &json!({"site": "sc-domain:example.com"}),
            None,
            "none",
            &OutputFormat::Json,
        );
        let v: Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(v["site"], "sc-domain:example.com");
        assert_eq!(v["count"], 1);
        assert_eq!(v["columns"], json!(["query", "clicks", "ctr"]));
        assert_eq!(v["rows"][0]["clicks"], 12);
        assert!(v.get("note").is_none());
    }

    #[test]
    fn render_report_json_empty_has_note() {
        let r = render_report(
            &report_cols(),
            &[],
            &json!({}),
            None,
            "No data in range.",
            &OutputFormat::Json,
        );
        let v: Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(v["count"], 0);
        assert_eq!(v["rows"], json!([]));
        assert_eq!(v["note"], "No data in range.");
    }

    #[test]
    fn render_report_yaml_has_rows() {
        let rows = vec![vec![json!("q"), json!(1), json!("x")]];
        let r = render_report(
            &report_cols(),
            &rows,
            &json!({}),
            None,
            "",
            &OutputFormat::Yaml,
        );
        assert!(r.stdout.contains("count: 1"));
        assert!(r.stdout.contains("query: \"q\""));
    }
}

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
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "table" => Self::Table,
            "yaml" | "yml" => Self::Yaml,
            "csv" => Self::Csv,
            _ => Self::Json,
        }
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
/// For YAML the page separator is preserved as-is.
pub fn format_value_paginated(value: &Value, format: &OutputFormat, is_first_page: bool) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string(value).unwrap_or_default(),
        OutputFormat::Csv => format_csv_page(value, is_first_page),
        OutputFormat::Table => format_table_page(value, is_first_page),
        OutputFormat::Yaml => format_yaml(value),
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
        // Single object: key/value table
        let mut output = String::new();
        let max_key_len = obj.keys().map(|k| k.len()).max().unwrap_or(0);
        for (key, val) in obj {
            let val_str = value_to_cell(val);
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

    // Collect all unique keys across all objects
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

    if columns.is_empty() {
        // Array of non-objects
        let mut output = String::new();
        for item in arr {
            let _ = writeln!(output, "{}", value_to_cell(item));
        }
        return output;
    }

    // Calculate column widths
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    let rows: Vec<Vec<String>> = arr
        .iter()
        .map(|item| {
            columns
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    let cell = if let Value::Object(obj) = item {
                        obj.get(col).map(value_to_cell).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    if cell.len() > widths[i] {
                        widths[i] = cell.len();
                    }
                    // Cap column width at 60
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

    // Rows
    for row in &rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let truncated = if c.len() > widths[i] {
                    format!("{}…", &c[..widths[i] - 1])
                } else {
                    c.clone()
                };
                format!("{:width$}", truncated, width = widths[i])
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
    fn test_format_value_paginated_json_is_compact() {
        let val = json!({"files": [{"id": "1"}]});
        let output = format_value_paginated(&val, &OutputFormat::Json, true);
        // Compact JSON — no pretty-printed newlines inside the object
        assert!(
            !output.contains("\n  "),
            "JSON must be compact in paginated mode"
        );
    }
}

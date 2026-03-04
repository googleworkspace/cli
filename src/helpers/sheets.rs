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
use std::future::Future;
use std::pin::Pin;

pub struct SheetsHelper;

impl Helper for SheetsHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+append")
                .about("[Helper] Append a row to a spreadsheet")
                .arg(
                    Arg::new("spreadsheet")
                        .long("spreadsheet")
                        .help("Spreadsheet ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("values")
                        .long("values")
                        .help("Comma-separated values (simple strings)")
                        .value_name("VALUES"),
                )
                .arg(
                    Arg::new("json-values")
                        .long("json-values")
                        .help("JSON array of rows, e.g. '[[\"a\",\"b\"],[\"c\",\"d\"]]'")
                        .value_name("JSON"),
                )
                .after_help(
                    r#"EXAMPLES:
  gws sheets +append --spreadsheet ID --values 'Alice,100,true'
  gws sheets +append --spreadsheet ID --json-values '[["a","b"],["c","d"]]'

TIPS:
  Use --values for simple single-row appends.
  Use --json-values for bulk multi-row inserts."#,
                ),
        );

        cmd = cmd.subcommand(
            Command::new("+read")
                .about("[Helper] Read values from a spreadsheet")
                .arg(
                    Arg::new("spreadsheet")
                        .long("spreadsheet")
                        .help("Spreadsheet ID")
                        .required(true)
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("range")
                        .long("range")
                        .help("Range to read (e.g. 'Sheet1!A1:B2')")
                        .required(true)
                        .value_name("RANGE"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws sheets +read --spreadsheet ID --range 'Sheet1!A1:D10'
  gws sheets +read --spreadsheet ID --range Sheet1

TIPS:
  Read-only — never modifies the spreadsheet.
  For advanced options, use the raw values.get API.",
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
            if let Some(matches) = matches.subcommand_matches("+append") {
                let config = parse_append_args(matches);
                let (params_str, body_str, scopes) = build_append_request(&config, doc)?;

                let scope_strs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scope_strs).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) => (None, executor::AuthMethod::None),
                };

                let spreadsheets_res = doc.resources.get("spreadsheets").ok_or_else(|| {
                    GwsError::Discovery("Resource 'spreadsheets' not found".to_string())
                })?;
                let values_res = spreadsheets_res.resources.get("values").ok_or_else(|| {
                    GwsError::Discovery("Resource 'spreadsheets.values' not found".to_string())
                })?;
                let append_method = values_res.methods.get("append").ok_or_else(|| {
                    GwsError::Discovery("Method 'spreadsheets.values.append' not found".to_string())
                })?;

                let pagination = executor::PaginationConfig {
                    page_all: false,
                    page_limit: 10,
                    page_delay_ms: 100,
                };

                executor::execute_method(
                    doc,
                    append_method,
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
                    false
    )
                .await?;

                return Ok(true);
            }

            if let Some(matches) = matches.subcommand_matches("+read") {
                let config = parse_read_args(matches);
                let (params_str, scopes) = build_read_request(&config, doc)?;

                // Re-find method
                let spreadsheets_res = doc.resources.get("spreadsheets").ok_or_else(|| {
                    GwsError::Discovery("Resource 'spreadsheets' not found".to_string())
                })?;
                let values_res = spreadsheets_res.resources.get("values").ok_or_else(|| {
                    GwsError::Discovery("Resource 'spreadsheets.values' not found".to_string())
                })?;
                let get_method = values_res.methods.get("get").ok_or_else(|| {
                    GwsError::Discovery("Method 'spreadsheets.values.get' not found".to_string())
                })?;

                let scope_strs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scope_strs).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) => (None, executor::AuthMethod::None),
                };

                executor::execute_method(
                    doc,
                    get_method,
                    Some(&params_str),
                    None,
                    token.as_deref(),
                    auth_method,
                    None,
                    None,
                    matches.get_flag("dry-run"),
                    &executor::PaginationConfig::default(),
                    None,
                    &crate::helpers::modelarmor::SanitizeMode::Warn,
                    &crate::formatter::OutputFormat::default(),
                    false
    )
                .await?;

                return Ok(true);
            }

            Ok(false)
        })
    }
}

fn build_append_request(
    config: &AppendConfig,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let spreadsheets_res = doc
        .resources
        .get("spreadsheets")
        .ok_or_else(|| GwsError::Discovery("Resource 'spreadsheets' not found".to_string()))?;
    let values_res = spreadsheets_res.resources.get("values").ok_or_else(|| {
        GwsError::Discovery("Resource 'spreadsheets.values' not found".to_string())
    })?;
    let append_method = values_res.methods.get("append").ok_or_else(|| {
        GwsError::Discovery("Method 'spreadsheets.values.append' not found".to_string())
    })?;

    let range = "A1";

    let params = json!({
        "spreadsheetId": config.spreadsheet_id,
        "range": range,
        "valueInputOption": "USER_ENTERED"
    });

    // We use `json!` macro to construct a generic JSON Value for the request body.
    // This allows us to easily create nested objects without defining explicit structs
    // for every API request body.
    let body = json!({
        "values": [config.values]
    });

    // Map `&String` scope URLs to owned `String`s for the return value
    let scopes: Vec<String> = append_method.scopes.iter().map(|s| s.to_string()).collect();

    Ok((params.to_string(), body.to_string(), scopes))
}

fn build_read_request(
    config: &ReadConfig,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, Vec<String>), GwsError> {
    // ... resource lookup omitted for brevity ...
    let spreadsheets_res = doc
        .resources
        .get("spreadsheets")
        .ok_or_else(|| GwsError::Discovery("Resource 'spreadsheets' not found".to_string()))?;
    let values_res = spreadsheets_res.resources.get("values").ok_or_else(|| {
        GwsError::Discovery("Resource 'spreadsheets.values' not found".to_string())
    })?;
    let get_method = values_res.methods.get("get").ok_or_else(|| {
        GwsError::Discovery("Method 'spreadsheets.values.get' not found".to_string())
    })?;

    let params = json!({
        "spreadsheetId": config.spreadsheet_id,
        "range": config.range
    });

    let scopes: Vec<String> = get_method.scopes.iter().map(|s| s.to_string()).collect();

    Ok((params.to_string(), scopes))
}

/// Configuration for appending values to a spreadsheet.
///
/// Holds the parsed arguments for the `+append` subcommand.
pub struct AppendConfig {
    /// The ID of the spreadsheet to append to.
    pub spreadsheet_id: String,
    /// The values to append, as a vector of strings.
    pub values: Vec<String>,
}

/// Parses arguments for the `+append` command.
///
/// Splits the comma-separated `values` argument into a `Vec<String>`.
pub fn parse_append_args(matches: &ArgMatches) -> AppendConfig {
    let values = if let Some(json_str) = matches.get_one::<String>("json-values") {
        // Parse JSON array of rows
        if let Ok(parsed) = serde_json::from_str::<Vec<Vec<String>>>(json_str) {
            parsed.into_iter().flatten().collect()
        } else {
            // Treat as single row JSON array
            serde_json::from_str::<Vec<String>>(json_str).unwrap_or_default()
        }
    } else if let Some(values_str) = matches.get_one::<String>("values") {
        values_str.split(',').map(|s| s.to_string()).collect()
    } else {
        Vec::new()
    };

    AppendConfig {
        spreadsheet_id: matches.get_one::<String>("spreadsheet").unwrap().clone(),
        values,
    }
}

/// Configuration for reading values from a spreadsheet.
pub struct ReadConfig {
    pub spreadsheet_id: String,
    /// A1 notation range (e.g. "Sheet1!A1:B2").
    pub range: String,
}

pub fn parse_read_args(matches: &ArgMatches) -> ReadConfig {
    ReadConfig {
        spreadsheet_id: matches.get_one::<String>("spreadsheet").unwrap().clone(),
        range: matches.get_one::<String>("range").unwrap().clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{RestDescription, RestMethod, RestResource};
    use std::collections::HashMap;

    fn make_mock_doc() -> RestDescription {
        let mut methods = HashMap::new();
        methods.insert(
            "append".to_string(),
            RestMethod {
                scopes: vec!["https://scope".to_string()],
                ..Default::default()
            },
        );
        methods.insert(
            "get".to_string(),
            RestMethod {
                scopes: vec!["https://scope".to_string()],
                ..Default::default()
            },
        );

        let mut values_res = RestResource::default();
        values_res.methods = methods;

        let mut spreadsheets_res = RestResource::default();
        spreadsheets_res
            .resources
            .insert("values".to_string(), values_res);

        let mut resources = HashMap::new();
        resources.insert("spreadsheets".to_string(), spreadsheets_res);

        RestDescription {
            resources,
            ..Default::default()
        }
    }

    fn make_matches_append(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("spreadsheet").long("spreadsheet"))
            .arg(Arg::new("values").long("values"))
            .arg(Arg::new("json-values").long("json-values"));
        cmd.try_get_matches_from(args).unwrap()
    }

    fn make_matches_read(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("spreadsheet").long("spreadsheet"))
            .arg(Arg::new("range").long("range"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_build_append_request() {
        let doc = make_mock_doc();
        let config = AppendConfig {
            spreadsheet_id: "123".to_string(),
            values: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        };
        let (params, body, scopes) = build_append_request(&config, &doc).unwrap();

        assert!(params.contains("123"));
        assert!(params.contains("USER_ENTERED"));
        assert!(body.contains("a"));
        assert!(body.contains("b"));
        assert_eq!(scopes[0], "https://scope");
    }

    #[test]
    fn test_build_read_request() {
        let doc = make_mock_doc();
        let config = ReadConfig {
            spreadsheet_id: "123".to_string(),
            range: "A1:B2".to_string(),
        };
        let (params, scopes) = build_read_request(&config, &doc).unwrap();

        assert!(params.contains("123"));
        assert!(params.contains("A1:B2"));
        assert_eq!(scopes[0], "https://scope");
    }

    #[test]
    fn test_parse_append_args() {
        let matches = make_matches_append(&["test", "--spreadsheet", "123", "--values", "a,b,c"]);
        let config = parse_append_args(&matches);
        assert_eq!(config.spreadsheet_id, "123");
        assert_eq!(config.values, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_read_args() {
        let matches = make_matches_read(&["test", "--spreadsheet", "123", "--range", "A1:B2"]);
        let config = parse_read_args(&matches);
        assert_eq!(config.spreadsheet_id, "123");
        assert_eq!(config.range, "A1:B2");
    }

    #[test]
    fn test_inject_commands() {
        let helper = SheetsHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();

        let cmd = helper.inject_commands(cmd, &doc);
        let subcommands: Vec<_> = cmd.get_subcommands().map(|s| s.get_name()).collect();
        assert!(subcommands.contains(&"+append"));
        assert!(subcommands.contains(&"+read"));
    }
}

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
use clap::{Arg, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct TasksHelper;

impl Helper for TasksHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+insert")
                .about("[Helper] Create a new task with optional due date/time")
                .arg(
                    Arg::new("tasklist")
                        .long("tasklist")
                        .help("Task list ID (default: @default)")
                        .default_value("@default")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("title")
                        .long("title")
                        .help("Task title")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("notes")
                        .long("notes")
                        .help("Task notes/description")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("due")
                        .long("due")
                        .help(
                            "Due date/time in RFC 3339 format (e.g. 2026-06-15 or \
                             2026-06-15T10:30:00Z). When a time component is provided \
                             it is preserved in the request; date-only values are sent \
                             with T00:00:00.000Z.",
                        )
                        .value_name("DATETIME"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws tasks +insert --title 'Buy milk'
  gws tasks +insert --title 'Call dentist' --due 2026-06-20
  gws tasks +insert --title 'Submit report' --due 2026-06-20T14:00:00Z
  gws tasks +insert --title 'Review PR' --notes 'See github.com/…' --tasklist MDc0NzQ5Mzg

TIPS:
  --due accepts either a plain date (YYYY-MM-DD) or a full RFC 3339 datetime.
  The Google Tasks API preserves the time component you supply.",
                ),
        );
        cmd
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(m) = matches.subcommand_matches("+insert") {
                handle_insert(m).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

async fn handle_insert(matches: &ArgMatches) -> Result<(), GwsError> {
    let scope = "https://www.googleapis.com/auth/tasks";
    let token = auth::get_token(&[scope])
        .await
        .map_err(|e| GwsError::Auth(format!("Tasks auth failed: {e}")))?;

    let tasklist = matches.get_one::<String>("tasklist").unwrap();
    let tasklist = crate::validate::validate_resource_name(tasklist)?;

    let title = matches.get_one::<String>("title").unwrap();
    let notes = matches.get_one::<String>("notes");
    let due_input = matches.get_one::<String>("due");

    let mut body = json!({ "title": title });

    if let Some(n) = notes {
        body["notes"] = json!(n);
    }

    if let Some(due_str) = due_input {
        let due_rfc3339 = normalize_due_date(due_str)?;
        body["due"] = json!(due_rfc3339);
    }

    let url = format!(
        "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks",
        tasklist,
    );

    let client = crate::client::build_client()?;
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to create task: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body_text,
            reason: "task_insert_failed".to_string(),
            enable_url: None,
        });
    }

    let result: Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse response: {e}")))?;

    println!(
        "{}",
        crate::formatter::format_value(&result, &crate::formatter::OutputFormat::default())
    );
    Ok(())
}

/// Normalize a user-supplied due date string into the RFC 3339 format expected
/// by the Google Tasks API.
///
/// Accepted inputs:
/// - `YYYY-MM-DD` — date only; converted to `YYYY-MM-DDT00:00:00.000Z`
/// - Any string containing an uppercase `T` separator — treated as a full RFC 3339
///   datetime and returned unchanged so the time component is **preserved**.
///
/// Returning the value verbatim for datetime strings (rather than re-parsing and
/// re-formatting) is intentional: it avoids silently truncating or normalising
/// the offset supplied by the caller (e.g. `+05:30` stays `+05:30`).
pub(crate) fn normalize_due_date(due_str: &str) -> Result<String, GwsError> {
    // Reject obviously invalid inputs early.
    if due_str.is_empty() {
        return Err(GwsError::Validation(
            "--due value must not be empty".to_string(),
        ));
    }

    // If the string already contains the RFC 3339 date/time separator ('T'),
    // the caller has supplied a full datetime — pass it through without
    // modification so that the time component (and its timezone offset) are
    // preserved.  Only the uppercase 'T' is recognised as a separator; a
    // lowercase 't' can appear in arbitrary invalid strings and should fall
    // through to the date-format validation below.
    if due_str.contains('T') {
        return Ok(due_str.to_string());
    }

    // Otherwise treat it as a plain date (YYYY-MM-DD) and append midnight UTC.
    // Validate that it looks like a date before appending.
    let parts: Vec<&str> = due_str.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || parts.iter().any(|p| p.parse::<u32>().is_err())
    {
        return Err(GwsError::Validation(format!(
            "--due value '{due_str}' is not a valid date (YYYY-MM-DD) or \
             datetime (RFC 3339, e.g. 2026-06-20T14:00:00Z)"
        )));
    }

    Ok(format!("{due_str}T00:00:00.000Z"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_only_gets_midnight_appended() {
        let result = normalize_due_date("2026-06-20").unwrap();
        assert_eq!(result, "2026-06-20T00:00:00.000Z");
    }

    #[test]
    fn test_datetime_with_utc_offset_is_preserved() {
        let input = "2026-06-20T14:30:00Z";
        let result = normalize_due_date(input).unwrap();
        assert_eq!(result, input, "time component must not be stripped");
    }

    #[test]
    fn test_datetime_with_named_offset_is_preserved() {
        let input = "2026-06-20T14:30:00+05:30";
        let result = normalize_due_date(input).unwrap();
        assert_eq!(result, input, "timezone offset must not be modified");
    }

    #[test]
    fn test_datetime_with_negative_offset_is_preserved() {
        let input = "2026-06-20T09:00:00-07:00";
        let result = normalize_due_date(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_datetime_with_milliseconds_is_preserved() {
        let input = "2026-06-20T14:30:00.000Z";
        let result = normalize_due_date(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_empty_string_returns_error() {
        let result = normalize_due_date("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_invalid_format_returns_error() {
        let result = normalize_due_date("not-a-date");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_date_format_returns_error() {
        let result = normalize_due_date("2026/06/20");
        assert!(result.is_err());
    }
}

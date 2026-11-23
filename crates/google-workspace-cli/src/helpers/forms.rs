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

//! `gws forms +responses`: a form's responses as one row per response, with
//! the question titles as column headers, in form order.

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::formatter::OutputFormat;
use clap::{Arg, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

const FORMS_BASE: &str = "https://forms.googleapis.com/v1";
const SCOPE_BODY: &str = "https://www.googleapis.com/auth/forms.body.readonly";
const SCOPE_RESPONSES: &str = "https://www.googleapis.com/auth/forms.responses.readonly";

pub struct FormsHelper;

impl Helper for FormsHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+responses")
                .about("[Helper] A form's responses: one row each, question titles as columns")
                .arg(
                    Arg::new("form")
                        .help("Form ID, or the form's edit URL (docs.google.com/forms/d/<id>/edit)")
                        .required(true)
                        .value_name("FORM"),
                )
                .arg(
                    Arg::new("since")
                        .long("since")
                        .help("Only responses submitted at or after this date (YYYY-MM-DD, local midnight) or RFC 3339 time")
                        .value_name("WHEN"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws forms +responses 1FAIpQLSf...edit-id
  gws forms +responses https://docs.google.com/forms/d/<id>/edit --format csv > responses.csv
  gws forms +responses <id> --since 2026-11-01
  gws forms +responses <id> --since 2026-11-20T09:00:00+01:00 --format json

TIPS:
  Read-only. Default output is a table; csv and json keep every column.
  Columns: submitted, email (if collected), score (if a quiz), then one per
  question. A grid question gives one column per row: 'Question [row]'.
  Several answers to one question are joined with '; '.
  The ID in a /forms/d/e/.../viewform link is the public one and won't work:
  open the form in edit mode and copy that URL instead.",
                ),
        )
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(m) = matches.subcommand_matches("+responses") {
                handle_responses(m).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

/// Extracts a form ID from a bare ID or a Google Forms edit URL.
pub(crate) fn parse_form_id(raw: &str) -> Result<String, GwsError> {
    crate::validate::reject_dangerous_chars(raw, "form")?;
    let raw = raw.trim();
    let candidate = if raw.contains("://") || raw.starts_with("docs.google.com") {
        let after = raw.split("/forms/d/").nth(1).ok_or_else(|| {
            GwsError::Validation(format!(
                "'{raw}' is not a Google Forms URL (expected .../forms/d/<id>/edit)"
            ))
        })?;
        if after.starts_with("e/") {
            return Err(GwsError::Validation(
                "That is the form's public link (/forms/d/e/...), whose ID the API can't use. \
                 Open the form in edit mode and pass that URL or its ID."
                    .to_string(),
            ));
        }
        after
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .to_string()
    } else {
        raw.to_string()
    };
    let ok = candidate.len() >= 10
        && candidate.len() <= 128
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(GwsError::Validation(format!(
            "Invalid form ID '{candidate}': expected letters, digits, '-' and '_'"
        )));
    }
    Ok(candidate)
}

/// Converts `--since` to the RFC 3339 UTC ("Zulu") timestamp the API's
/// filter requires. A bare date means local midnight in `tz`.
pub(crate) fn parse_since<Tz: chrono::TimeZone>(raw: &str, tz: &Tz) -> Result<String, GwsError> {
    let raw = raw.trim();
    let utc = if let Ok(t) = chrono::DateTime::parse_from_rfc3339(raw) {
        t.with_timezone(&chrono::Utc)
    } else if let Ok(d) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let midnight = d.and_hms_opt(0, 0, 0).expect("valid time");
        tz.from_local_datetime(&midnight)
            .earliest()
            .ok_or_else(|| {
                GwsError::Validation(format!("--since {raw} is not a valid local date"))
            })?
            .with_timezone(&chrono::Utc)
    } else {
        return Err(GwsError::Validation(format!(
            "--since must be a date (2026-11-01) or RFC 3339 time (2026-11-01T09:00:00+01:00); got '{raw}'"
        )));
    };
    Ok(utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// One answer column: the question ID it reads and its header.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QuestionColumn {
    pub question_id: String,
    pub header: String,
}

/// Question columns in form order. Grid questions contribute one column per
/// row; duplicate titles get " (2)", " (3)" so CSV headers stay unique.
pub(crate) fn question_columns(form: &Value) -> Vec<QuestionColumn> {
    let mut cols: Vec<QuestionColumn> = Vec::new();
    let items = form
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in &items {
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if let Some(qid) = item
            .pointer("/questionItem/question/questionId")
            .and_then(Value::as_str)
        {
            let header = if title.is_empty() {
                format!("Question {}", cols.len() + 1)
            } else {
                title.clone()
            };
            cols.push(QuestionColumn {
                question_id: qid.to_string(),
                header,
            });
        } else if let Some(questions) = item
            .pointer("/questionGroupItem/questions")
            .and_then(Value::as_array)
        {
            for q in questions {
                let Some(qid) = q.get("questionId").and_then(Value::as_str) else {
                    continue;
                };
                let row = q
                    .pointer("/rowQuestion/title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let header = match (title.is_empty(), row.is_empty()) {
                    (false, false) => format!("{title} [{row}]"),
                    (false, true) => title.clone(),
                    (true, false) => row.to_string(),
                    (true, true) => format!("Question {}", cols.len() + 1),
                };
                cols.push(QuestionColumn {
                    question_id: qid.to_string(),
                    header,
                });
            }
        }
    }
    dedupe_headers(&mut cols);
    cols
}

fn dedupe_headers(cols: &mut [QuestionColumn]) {
    let reserved = ["submitted", "email", "score", "response_id"];
    let mut seen: Vec<String> = reserved.iter().map(|s| s.to_string()).collect();
    for c in cols.iter_mut() {
        let base = c.header.clone();
        let mut name = base.clone();
        let mut n = 2;
        while seen.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            name = format!("{base} ({n})");
            n += 1;
        }
        seen.push(name.clone());
        c.header = name;
    }
}

/// The text of one answer: text answers (several joined with "; "), or
/// uploaded files by name, with the grade appended for quiz questions.
pub(crate) fn answer_text(answer: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(arr) = answer
        .pointer("/textAnswers/answers")
        .and_then(Value::as_array)
    {
        parts.extend(
            arr.iter()
                .filter_map(|a| a.get("value").and_then(Value::as_str))
                .map(str::to_string),
        );
    }
    if let Some(arr) = answer
        .pointer("/fileUploadAnswers/answers")
        .and_then(Value::as_array)
    {
        parts.extend(arr.iter().map(|f| {
            let name = f.get("fileName").and_then(Value::as_str).unwrap_or("file");
            match f.get("fileId").and_then(Value::as_str) {
                Some(id) => format!("{name} (https://drive.google.com/open?id={id})"),
                None => name.to_string(),
            }
        }));
    }
    parts.join("; ")
}

/// Builds the columns and one row per response, oldest first.
pub(crate) fn build_rows(
    questions: &[QuestionColumn],
    responses: &[Value],
) -> (Vec<String>, Vec<Vec<Value>>) {
    let has_email = responses
        .iter()
        .any(|r| r.get("respondentEmail").and_then(Value::as_str).is_some());
    let has_score = responses.iter().any(|r| r.get("totalScore").is_some());
    let mut columns = vec!["submitted".to_string()];
    if has_email {
        columns.push("email".to_string());
    }
    if has_score {
        columns.push("score".to_string());
    }
    columns.extend(questions.iter().map(|q| q.header.clone()));
    columns.push("response_id".to_string());

    let mut sorted: Vec<&Value> = responses.iter().collect();
    let ts = |r: &Value| {
        r.get("lastSubmittedTime")
            .or_else(|| r.get("createTime"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    sorted.sort_by_key(|r| ts(r));

    let rows = sorted
        .into_iter()
        .map(|r| {
            let mut row = vec![json!(ts(r))];
            if has_email {
                row.push(json!(r
                    .get("respondentEmail")
                    .and_then(Value::as_str)
                    .unwrap_or("")));
            }
            if has_score {
                row.push(r.get("totalScore").cloned().unwrap_or(json!("")));
            }
            let answers = r.get("answers");
            for q in questions {
                let text = answers
                    .and_then(|a| a.get(&q.question_id))
                    .map(answer_text)
                    .unwrap_or_default();
                row.push(json!(text));
            }
            row.push(json!(r
                .get("responseId")
                .and_then(Value::as_str)
                .unwrap_or("")));
            row
        })
        .collect();
    (columns, rows)
}

async fn handle_responses(matches: &ArgMatches) -> Result<(), GwsError> {
    let form_id = parse_form_id(matches.get_one::<String>("form").expect("required"))?;
    let since = matches
        .get_one::<String>("since")
        .map(|s| parse_since(s, &chrono::Local))
        .transpose()?;
    let format = match matches.get_one::<String>("format") {
        Some(f) => OutputFormat::from_str(f),
        None => OutputFormat::Table,
    };
    let encoded = crate::validate::encode_path_segment(&form_id);
    let form_url = format!("{FORMS_BASE}/forms/{encoded}");
    let responses_url = format!("{FORMS_BASE}/forms/{encoded}/responses");
    let filter = since.as_ref().map(|s| format!("timestamp >= {s}"));

    if matches.get_flag("dry-run") {
        let mut q = json!({"pageSize": 5000});
        if let Some(f) = &filter {
            q["filter"] = json!(f);
        }
        let out = json!({"dry_run": true, "requests": [
            {"method": "GET", "url": form_url},
            {"method": "GET", "url": responses_url, "query": q},
        ]});
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return Ok(());
    }

    let token = auth::get_token(&[SCOPE_BODY, SCOPE_RESPONSES])
        .await
        .map_err(|e| GwsError::Auth(format!("Forms auth failed: {e}")))?;
    let client = crate::client::shared_client()?;

    let form =
        crate::executor::fetch_json(|| client.get(&form_url).bearer_auth(&token), "Forms get")
            .await?;

    let mut responses: Vec<Value> = Vec::new();
    let mut page: Option<String> = None;
    // 5000 per page; 200 pages covers a million responses.
    for _ in 0..200 {
        let resp = crate::executor::fetch_json(
            || {
                let mut q: Vec<(&str, String)> = vec![("pageSize", "5000".to_string())];
                if let Some(f) = &filter {
                    q.push(("filter", f.clone()));
                }
                if let Some(p) = &page {
                    q.push(("pageToken", p.clone()));
                }
                client.get(&responses_url).query(&q).bearer_auth(&token)
            },
            "Forms responses.list",
        )
        .await?;
        if let Some(items) = resp.get("responses").and_then(Value::as_array) {
            responses.extend(items.iter().cloned());
        }
        page = resp
            .get("nextPageToken")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        if page.is_none() {
            break;
        }
    }

    let title = form
        .pointer("/info/title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let questions = question_columns(&form);
    let (columns, rows) = build_rows(&questions, &responses);
    let mut heading = format!("{title} · {} responses", rows.len());
    if let Some(s) = &since {
        heading.push_str(&format!(" since {s}"));
    }
    let empty = match &since {
        Some(s) => format!("No responses to '{title}' since {s}."),
        None => format!("No responses to '{title}' yet."),
    };
    let meta = json!({
        "formId": form_id,
        "title": title,
        "since": since,
        "responderUri": form.get("responderUri").cloned().unwrap_or(Value::Null),
    });
    let rendered =
        crate::formatter::render_report(&columns, &rows, &meta, Some(&heading), &empty, &format);
    print!("{}", rendered.stdout);
    if let Some(n) = rendered.notice {
        eprintln!("{n}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "1AbCdEfGhIjKlMnOpQrStUvWxYz_0123456789-xyz";

    #[test]
    fn form_id_from_id_and_urls() {
        assert_eq!(parse_form_id(ID).unwrap(), ID);
        assert_eq!(
            parse_form_id(&format!("https://docs.google.com/forms/d/{ID}/edit")).unwrap(),
            ID
        );
        assert_eq!(
            parse_form_id(&format!(
                "https://docs.google.com/forms/d/{ID}/edit?usp=sharing#responses"
            ))
            .unwrap(),
            ID
        );
        assert_eq!(
            parse_form_id(&format!("docs.google.com/forms/d/{ID}")).unwrap(),
            ID
        );
    }

    #[test]
    fn form_id_rejects_public_and_bad_input() {
        let err = parse_form_id("https://docs.google.com/forms/d/e/1FAIpQLSabc/viewform")
            .unwrap_err()
            .to_string();
        assert!(err.contains("public link"), "{err}");
        assert!(parse_form_id("https://example.com/x").is_err());
        assert!(parse_form_id("../../etc/passwd").is_err());
        assert!(parse_form_id("short").is_err());
        assert!(parse_form_id("abc\u{0}defghijk").is_err());
    }

    #[test]
    fn since_dates_and_times() {
        let wat = chrono::FixedOffset::east_opt(3600).unwrap();
        assert_eq!(
            parse_since("2026-11-01", &wat).unwrap(),
            "2026-10-31T23:00:00Z"
        );
        assert_eq!(
            parse_since("2026-11-20T09:00:00+01:00", &wat).unwrap(),
            "2026-11-20T08:00:00Z"
        );
        assert!(parse_since("last week", &wat).is_err());
        assert!(parse_since("2026-02-30", &wat).is_err());
    }

    fn sample_form() -> Value {
        json!({
            "info": {"title": "Feedback"},
            "items": [
                {"title": "Name", "questionItem": {"question": {"questionId": "q1"}}},
                {"title": "Section", "pageBreakItem": {}},
                {"title": "Rate us", "questionGroupItem": {"questions": [
                    {"questionId": "g1", "rowQuestion": {"title": "Speed"}},
                    {"questionId": "g2", "rowQuestion": {"title": "Accuracy"}}
                ]}},
                {"title": "Name", "questionItem": {"question": {"questionId": "q2"}}},
                {"title": "", "questionItem": {"question": {"questionId": "q3"}}},
                {"title": "Email", "questionItem": {"question": {"questionId": "q4"}}}
            ]
        })
    }

    #[test]
    fn question_columns_in_form_order() {
        let cols = question_columns(&sample_form());
        let headers: Vec<&str> = cols.iter().map(|c| c.header.as_str()).collect();
        assert_eq!(
            headers,
            vec![
                "Name",
                "Rate us [Speed]",
                "Rate us [Accuracy]",
                "Name (2)",
                "Question 5",
                "Email (2)"
            ]
        );
        assert_eq!(cols[1].question_id, "g1");
        assert!(question_columns(&json!({})).is_empty());
    }

    #[test]
    fn answers_text_files_and_multi() {
        assert_eq!(
            answer_text(&json!({"textAnswers": {"answers": [{"value": "A"}, {"value": "B"}]}})),
            "A; B"
        );
        assert_eq!(
            answer_text(
                &json!({"fileUploadAnswers": {"answers": [{"fileId": "f1", "fileName": "cv.pdf"}]}})
            ),
            "cv.pdf (https://drive.google.com/open?id=f1)"
        );
        assert_eq!(answer_text(&json!({})), "");
    }

    #[test]
    fn rows_sorted_with_optional_columns() {
        let cols = question_columns(&sample_form());
        let responses = vec![
            json!({"responseId": "r2", "lastSubmittedTime": "2026-11-21T10:00:00Z",
                   "respondentEmail": "b@x.com", "totalScore": 3,
                   "answers": {"q1": {"textAnswers": {"answers": [{"value": "Bo"}]}},
                               "g2": {"textAnswers": {"answers": [{"value": "5"}]}}}}),
            json!({"responseId": "r1", "lastSubmittedTime": "2026-11-20T10:00:00Z",
                   "answers": {"q1": {"textAnswers": {"answers": [{"value": "Al"}]}}}}),
        ];
        let (columns, rows) = build_rows(&cols, &responses);
        assert_eq!(&columns[..4], &["submitted", "email", "score", "Name"]);
        assert_eq!(columns.last().unwrap(), "response_id");
        assert_eq!(rows[0][0], "2026-11-20T10:00:00Z");
        assert_eq!(rows[0][1], "");
        assert_eq!(rows[0][3], "Al");
        assert_eq!(rows[1][1], "b@x.com");
        assert_eq!(rows[1][2], 3);
        assert_eq!(rows[1][5], "5");
        assert_eq!(rows[1].last().unwrap(), "r2");
    }

    #[test]
    fn rows_without_email_or_score() {
        let cols = question_columns(&sample_form());
        let (columns, rows) = build_rows(
            &cols,
            &[json!({"responseId": "r", "createTime": "2026-11-20T10:00:00Z"})],
        );
        assert_eq!(columns[1], "Name");
        assert_eq!(rows[0][0], "2026-11-20T10:00:00Z");
        let (_, rows) = build_rows(&cols, &[]);
        assert!(rows.is_empty());
    }

    #[test]
    fn csv_output_keeps_question_order() {
        let cols = question_columns(&sample_form());
        let (columns, rows) = build_rows(
            &cols,
            &[json!({"responseId": "r1", "lastSubmittedTime": "t",
                     "answers": {"q1": {"textAnswers": {"answers": [{"value": "Al, Jr"}]}}}})],
        );
        let out = crate::formatter::render_report(
            &columns,
            &rows,
            &json!({}),
            None,
            "",
            &OutputFormat::Csv,
        );
        let mut lines = out.stdout.lines();
        assert_eq!(
            lines.next().unwrap(),
            "submitted,Name,Rate us [Speed],Rate us [Accuracy],Name (2),Question 5,Email (2),response_id"
        );
        assert_eq!(lines.next().unwrap(), "t,\"Al, Jr\",,,,,,r1");
    }
}

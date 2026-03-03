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

//! Cross-service workflow helpers that compose multiple Google Workspace API
//! calls into high-level productivity actions.

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use clap::{Arg, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct WorkflowHelper;

impl Helper for WorkflowHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(build_standup_report_cmd());
        cmd = cmd.subcommand(build_meeting_prep_cmd());
        cmd = cmd.subcommand(build_email_to_task_cmd());
        cmd = cmd.subcommand(build_weekly_digest_cmd());
        cmd = cmd.subcommand(build_file_announce_cmd());
        cmd
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(m) = matches.subcommand_matches("+standup-report") {
                handle_standup_report(m).await?;
                return Ok(true);
            }
            if let Some(m) = matches.subcommand_matches("+meeting-prep") {
                handle_meeting_prep(m).await?;
                return Ok(true);
            }
            if let Some(m) = matches.subcommand_matches("+email-to-task") {
                handle_email_to_task(m).await?;
                return Ok(true);
            }
            if let Some(m) = matches.subcommand_matches("+weekly-digest") {
                handle_weekly_digest(m).await?;
                return Ok(true);
            }
            if let Some(m) = matches.subcommand_matches("+file-announce") {
                handle_file_announce(m).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }

    fn helper_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Command definitions
// ---------------------------------------------------------------------------

fn build_standup_report_cmd() -> Command {
    Command::new("+standup-report")
        .about("[Helper] Today's meetings + open tasks as a standup summary")
        .arg(
            Arg::new("format")
                .long("format")
                .help("Output format: json (default), table, yaml, csv")
                .value_name("FORMAT")
                .global(true),
        )
        .after_help(
            "\
EXAMPLES:
  gws workflow +standup-report
  gws workflow +standup-report --format table

TIPS:
  Read-only — never modifies data.
  Combines calendar agenda (today) with tasks list.",
        )
}

fn build_meeting_prep_cmd() -> Command {
    Command::new("+meeting-prep")
        .about("[Helper] Prepare for your next meeting: agenda, attendees, and linked docs")
        .arg(
            Arg::new("calendar")
                .long("calendar")
                .help("Calendar ID (default: primary)")
                .default_value("primary")
                .value_name("ID"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .help("Output format: json (default), table, yaml, csv")
                .value_name("FORMAT")
                .global(true),
        )
        .after_help(
            "\
EXAMPLES:
  gws workflow +meeting-prep
  gws workflow +meeting-prep --calendar Work

TIPS:
  Read-only — never modifies data.
  Shows the next upcoming event with attendees and description.",
        )
}

fn build_email_to_task_cmd() -> Command {
    Command::new("+email-to-task")
        .about("[Helper] Convert a Gmail message into a Google Tasks entry")
        .arg(
            Arg::new("message-id")
                .long("message-id")
                .help("Gmail message ID to convert")
                .required(true)
                .value_name("ID"),
        )
        .arg(
            Arg::new("tasklist")
                .long("tasklist")
                .help("Task list ID (default: @default)")
                .default_value("@default")
                .value_name("ID"),
        )
        .after_help(
            "\
EXAMPLES:
  gws workflow +email-to-task --message-id MSG_ID
  gws workflow +email-to-task --message-id MSG_ID --tasklist LIST_ID

TIPS:
  Reads the email subject as the task title and snippet as notes.
  Creates a new task — confirm with the user before executing.",
        )
}

fn build_weekly_digest_cmd() -> Command {
    Command::new("+weekly-digest")
        .about("[Helper] Weekly summary: this week's meetings + unread email count")
        .arg(
            Arg::new("format")
                .long("format")
                .help("Output format: json (default), table, yaml, csv")
                .value_name("FORMAT")
                .global(true),
        )
        .after_help(
            "\
EXAMPLES:
  gws workflow +weekly-digest
  gws workflow +weekly-digest --format table

TIPS:
  Read-only — never modifies data.
  Combines calendar agenda (week) with gmail triage summary.",
        )
}

fn build_file_announce_cmd() -> Command {
    Command::new("+file-announce")
        .about("[Helper] Announce a Drive file in a Chat space")
        .arg(
            Arg::new("file-id")
                .long("file-id")
                .help("Drive file ID to announce")
                .required(true)
                .value_name("ID"),
        )
        .arg(
            Arg::new("space")
                .long("space")
                .help("Chat space name (e.g. spaces/SPACE_ID)")
                .required(true)
                .value_name("SPACE"),
        )
        .arg(
            Arg::new("message")
                .long("message")
                .help("Custom announcement message")
                .value_name("TEXT"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .help("Output format: json (default), table, yaml, csv")
                .value_name("FORMAT")
                .global(true),
        )
        .after_help(
            "\
EXAMPLES:
  gws workflow +file-announce --file-id FILE_ID --space spaces/ABC123
  gws workflow +file-announce --file-id FILE_ID --space spaces/ABC123 --message 'Check this out!'

TIPS:
  This is a write command — sends a Chat message.
  Use `gws drive +upload` first to upload the file, then announce it here.
  Fetches the file name from Drive to build the announcement.",
        )
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn get_json(client: &reqwest::Client, url: &str, token: &str) -> Result<Value, GwsError> {
    let resp = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("HTTP request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "workflow_request_failed".to_string(),
        });
    }

    resp.json::<Value>()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("JSON parse failed: {e}")))
}

fn format_and_print(value: &Value, matches: &ArgMatches) {
    let fmt = matches
        .get_one::<String>("format")
        .map(|s| crate::formatter::OutputFormat::from_str(s))
        .unwrap_or_default();
    println!("{}", crate::formatter::format_value(value, &fmt));
}

async fn handle_standup_report(matches: &ArgMatches) -> Result<(), GwsError> {
    let cal_scope = "https://www.googleapis.com/auth/calendar.readonly";
    let tasks_scope = "https://www.googleapis.com/auth/tasks.readonly";
    let token = auth::get_token(&[cal_scope, tasks_scope])
        .await
        .map_err(|e| GwsError::Auth(format!("Auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    // Today's time range
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let day_start = (now / 86400) * 86400;
    let day_end = day_start + 86400;
    let time_min = epoch_to_rfc3339(day_start);
    let time_max = epoch_to_rfc3339(day_end);

    // Fetch today's events
    let events_url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=25",
        urlencoded(&time_min),
        urlencoded(&time_max),
    );
    let events_json = get_json(&client, &events_url, &token)
        .await
        .unwrap_or(json!({}));
    let events = events_json
        .get("items")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();

    let meetings: Vec<Value> = events
        .iter()
        .map(|e| {
            json!({
                "summary": e.get("summary").and_then(|v| v.as_str()).unwrap_or("(No title)"),
                "start": e.get("start").and_then(|s| s.get("dateTime").or(s.get("date"))).and_then(|v| v.as_str()).unwrap_or(""),
                "end": e.get("end").and_then(|s| s.get("dateTime").or(s.get("date"))).and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    // Fetch open tasks
    let tasks_url = "https://tasks.googleapis.com/tasks/v1/lists/@default/tasks?showCompleted=false&maxResults=20";
    let tasks_json = get_json(&client, tasks_url, &token)
        .await
        .unwrap_or(json!({}));
    let tasks = tasks_json
        .get("items")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();

    let open_tasks: Vec<Value> = tasks
        .iter()
        .map(|t| {
            json!({
                "title": t.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                "due": t.get("due").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    let output = json!({
        "meetings": meetings,
        "meetingCount": meetings.len(),
        "tasks": open_tasks,
        "taskCount": open_tasks.len(),
        "date": time_min.split('T').next().unwrap_or(""),
    });

    format_and_print(&output, matches);
    Ok(())
}

async fn handle_meeting_prep(matches: &ArgMatches) -> Result<(), GwsError> {
    let cal_scope = "https://www.googleapis.com/auth/calendar.readonly";
    let token = auth::get_token(&[cal_scope])
        .await
        .map_err(|e| GwsError::Auth(format!("Auth failed: {e}")))?;

    let client = crate::client::build_client()?;
    let calendar_id = matches
        .get_one::<String>("calendar")
        .map(|s| s.as_str())
        .unwrap_or("primary");

    // Fetch next upcoming event
    let now_rfc = epoch_to_rfc3339(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );

    let events_url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/{}/events?timeMin={}&singleEvents=true&orderBy=startTime&maxResults=1",
        urlencoded(calendar_id),
        urlencoded(&now_rfc),
    );
    let events_json = get_json(&client, &events_url, &token).await?;
    let items = events_json
        .get("items")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();

    if items.is_empty() {
        let output = json!({ "message": "No upcoming meetings found." });
        format_and_print(&output, matches);
        return Ok(());
    }

    let event = &items[0];
    let attendees = event
        .get("attendees")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();

    let attendee_list: Vec<Value> = attendees
        .iter()
        .map(|a| {
            json!({
                "email": a.get("email").and_then(|v| v.as_str()).unwrap_or(""),
                "responseStatus": a.get("responseStatus").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    let output = json!({
        "summary": event.get("summary").and_then(|v| v.as_str()).unwrap_or("(No title)"),
        "start": event.get("start").and_then(|s| s.get("dateTime").or(s.get("date"))).and_then(|v| v.as_str()).unwrap_or(""),
        "end": event.get("end").and_then(|s| s.get("dateTime").or(s.get("date"))).and_then(|v| v.as_str()).unwrap_or(""),
        "description": event.get("description").and_then(|v| v.as_str()).unwrap_or(""),
        "location": event.get("location").and_then(|v| v.as_str()).unwrap_or(""),
        "hangoutLink": event.get("hangoutLink").and_then(|v| v.as_str()).unwrap_or(""),
        "htmlLink": event.get("htmlLink").and_then(|v| v.as_str()).unwrap_or(""),
        "attendees": attendee_list,
        "attendeeCount": attendee_list.len(),
    });

    format_and_print(&output, matches);
    Ok(())
}

async fn handle_email_to_task(matches: &ArgMatches) -> Result<(), GwsError> {
    let gmail_scope = "https://www.googleapis.com/auth/gmail.readonly";
    let tasks_scope = "https://www.googleapis.com/auth/tasks";
    let token = auth::get_token(&[gmail_scope, tasks_scope])
        .await
        .map_err(|e| GwsError::Auth(format!("Auth failed: {e}")))?;

    let client = crate::client::build_client()?;
    let message_id = matches.get_one::<String>("message-id").unwrap();
    let tasklist = matches
        .get_one::<String>("tasklist")
        .map(|s| s.as_str())
        .unwrap_or("@default");

    // 1. Fetch the email
    let msg_url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=Subject",
        message_id,
    );
    let msg_json = get_json(&client, &msg_url, &token).await?;

    let subject = msg_json
        .get("payload")
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array())
        .and_then(|headers| {
            headers.iter().find(|h| {
                h.get("name")
                    .and_then(|n| n.as_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("Subject"))
            })
        })
        .and_then(|h| h.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("(No subject)");

    let snippet = msg_json
        .get("snippet")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // 2. Create the task
    let task_body = json!({
        "title": subject,
        "notes": format!("From email: {}\n\n{}", message_id, snippet),
    });

    let task_url = format!(
        "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks",
        tasklist,
    );

    let resp = client
        .post(&task_url)
        .bearer_auth(&token)
        .json(&task_body)
        .send()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to create task: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "task_create_failed".to_string(),
        });
    }

    let task_result: Value = resp.json().await.unwrap_or(json!({}));
    let output = json!({
        "created": true,
        "taskId": task_result.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "title": subject,
        "sourceMessageId": message_id,
    });

    format_and_print(&output, matches);
    Ok(())
}

async fn handle_weekly_digest(matches: &ArgMatches) -> Result<(), GwsError> {
    let cal_scope = "https://www.googleapis.com/auth/calendar.readonly";
    let gmail_scope = "https://www.googleapis.com/auth/gmail.readonly";
    let token = auth::get_token(&[cal_scope, gmail_scope])
        .await
        .map_err(|e| GwsError::Auth(format!("Auth failed: {e}")))?;

    let client = crate::client::build_client()?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let week_end = now + 7 * 86400;
    let time_min = epoch_to_rfc3339(now);
    let time_max = epoch_to_rfc3339(week_end);

    // Fetch this week's events
    let events_url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=50",
        urlencoded(&time_min),
        urlencoded(&time_max),
    );
    let events_json = get_json(&client, &events_url, &token)
        .await
        .unwrap_or(json!({}));
    let events = events_json
        .get("items")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();

    let meetings: Vec<Value> = events
        .iter()
        .map(|e| {
            json!({
                "summary": e.get("summary").and_then(|v| v.as_str()).unwrap_or("(No title)"),
                "start": e.get("start").and_then(|s| s.get("dateTime").or(s.get("date"))).and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    // Fetch unread email count
    let gmail_url =
        "https://gmail.googleapis.com/gmail/v1/users/me/messages?q=is%3Aunread&maxResults=1";
    let gmail_json = get_json(&client, gmail_url, &token)
        .await
        .unwrap_or(json!({}));
    let unread_estimate = gmail_json
        .get("resultSizeEstimate")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let output = json!({
        "meetings": meetings,
        "meetingCount": meetings.len(),
        "unreadEmails": unread_estimate,
        "periodStart": time_min,
        "periodEnd": time_max,
    });

    format_and_print(&output, matches);
    Ok(())
}

async fn handle_file_announce(matches: &ArgMatches) -> Result<(), GwsError> {
    let drive_scope = "https://www.googleapis.com/auth/drive.readonly";
    let chat_scope = "https://www.googleapis.com/auth/chat.messages.create";
    let token = auth::get_token(&[drive_scope, chat_scope])
        .await
        .map_err(|e| GwsError::Auth(format!("Auth failed: {e}")))?;

    let client = crate::client::build_client()?;
    let file_id = matches.get_one::<String>("file-id").unwrap();
    let space = matches.get_one::<String>("space").unwrap();
    let custom_msg = matches.get_one::<String>("message");

    // 1. Fetch file metadata from Drive
    let file_url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?fields=id,name,webViewLink",
        file_id,
    );
    let file_json = get_json(&client, &file_url, &token).await?;
    let file_name = file_json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("file");
    let default_link = format!("https://drive.google.com/file/d/{}/view", file_id);
    let file_link = file_json
        .get("webViewLink")
        .and_then(|v| v.as_str())
        .unwrap_or(&default_link);

    // 2. Send Chat message
    let msg_text = custom_msg
        .map(|m| format!("{m}\n{file_link}"))
        .unwrap_or_else(|| format!("📎 {file_name}\n{file_link}"));

    let chat_body = json!({ "text": msg_text });
    let chat_url = format!("https://chat.googleapis.com/v1/{}/messages", space);

    let chat_resp = client
        .post(&chat_url)
        .bearer_auth(&token)
        .json(&chat_body)
        .send()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Chat send failed: {e}")))?;

    if !chat_resp.status().is_success() {
        let status = chat_resp.status();
        let body = chat_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "chat_send_failed".to_string(),
        });
    }

    let output = json!({
        "announced": true,
        "fileName": file_name,
        "fileLink": file_link,
        "space": space,
    });

    format_and_print(&output, matches);
    Ok(())
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn epoch_to_rfc3339(epoch: u64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_opt(epoch as i64, 0).unwrap().to_rfc3339()
}

fn urlencoded(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('@', "%40")
        .replace('+', "%2B")
        .replace(':', "%3A")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_commands() {
        let helper = WorkflowHelper;
        let cmd = Command::new("test");
        let doc = crate::discovery::RestDescription::default();
        let cmd = helper.inject_commands(cmd, &doc);
        let names: Vec<_> = cmd
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        assert!(names.contains(&"+standup-report".to_string()));
        assert!(names.contains(&"+meeting-prep".to_string()));
        assert!(names.contains(&"+email-to-task".to_string()));
        assert!(names.contains(&"+weekly-digest".to_string()));
        assert!(names.contains(&"+file-announce".to_string()));
    }

    #[test]
    fn test_helper_only() {
        assert!(WorkflowHelper.helper_only());
    }
}

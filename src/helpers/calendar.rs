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
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::json;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

pub struct CalendarHelper;

impl Helper for CalendarHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+insert")
                .about("[Helper] create a new event")
                .arg(
                    Arg::new("calendar")
                        .long("calendar")
                        .help("Calendar ID (default: primary)")
                        .default_value("primary")
                        .value_name("ID"),
                )
                .arg(
                    Arg::new("summary")
                        .long("summary")
                        .help("Event summary/title")
                        .required(true)
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("start")
                        .long("start")
                        .help("Start time (ISO 8601, e.g., 2024-01-01T10:00:00Z)")
                        .required(true)
                        .value_name("TIME"),
                )
                .arg(
                    Arg::new("end")
                        .long("end")
                        .help("End time (ISO 8601)")
                        .required(true)
                        .value_name("TIME"),
                )
                .arg(
                    Arg::new("location")
                        .long("location")
                        .help("Event location")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("description")
                        .long("description")
                        .help("Event description/body")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("attendee")
                        .long("attendee")
                        .help("Attendee email (can be used multiple times)")
                        .value_name("EMAIL")
                        .action(ArgAction::Append),
                )
                .arg(
                    Arg::new("meet")
                        .long("meet")
                        .help("Add a Google Meet video conference link")
                        .action(ArgAction::SetTrue),
                )
                .after_help("\
EXAMPLES:
  gws calendar +insert --summary 'Standup' --start '2026-06-17T09:00:00-07:00' --end '2026-06-17T09:30:00-07:00'
  gws calendar +insert --summary 'Review' --start ... --end ... --attendee alice@example.com
  gws calendar +insert --summary 'Meet' --start ... --end ... --meet

TIPS:
  Use RFC3339 format for times (e.g. 2026-06-17T09:00:00-07:00).
  The --meet flag automatically adds a Google Meet link to the event."),
        );
        cmd = cmd.subcommand(
            Command::new("+agenda")
                .about("[Helper] Show upcoming events across all calendars")
                .arg(
                    Arg::new("today")
                        .long("today")
                        .help("Show today's events")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("tomorrow")
                        .long("tomorrow")
                        .help("Show tomorrow's events")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("week")
                        .long("week")
                        .help("Show this week's events")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("days")
                        .long("days")
                        .help("Number of days ahead to show")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("calendar")
                        .long("calendar")
                        .help("Filter to specific calendar name or ID")
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("timezone")
                        .long("timezone")
                        .alias("tz")
                        .help("IANA timezone override (e.g. America/Denver). Defaults to Google account timezone.")
                        .value_name("TZ"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws calendar +agenda
  gws calendar +agenda --today
  gws calendar +agenda --week --format table
  gws calendar +agenda --days 3 --calendar 'Work'
  gws calendar +agenda --today --timezone America/New_York

TIPS:
  Read-only — never modifies events.
  Queries all calendars by default; use --calendar to filter.
  Uses your Google account timezone by default; override with --timezone.",
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
            if let Some(matches) = matches.subcommand_matches("+insert") {
                let (params_str, body_str, scopes) = build_insert_request(matches, doc)?;

                let scopes_str: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
                let (token, auth_method) = match auth::get_token(&scopes_str).await {
                    Ok(t) => (Some(t), executor::AuthMethod::OAuth),
                    Err(_) if matches.get_flag("dry-run") => (None, executor::AuthMethod::None),
                    Err(e) => return Err(GwsError::Auth(format!("Calendar auth failed: {e}"))),
                };

                let events_res = doc.resources.get("events").ok_or_else(|| {
                    GwsError::Discovery("Resource 'events' not found".to_string())
                })?;
                let insert_method = events_res.methods.get("insert").ok_or_else(|| {
                    GwsError::Discovery("Method 'events.insert' not found".to_string())
                })?;

                executor::execute_method(
                    doc,
                    insert_method,
                    Some(&params_str),
                    Some(&body_str),
                    token.as_deref(),
                    auth_method,
                    None,
                    None,
                    matches.get_flag("dry-run"),
                    &executor::PaginationConfig::default(),
                    None,
                    &crate::helpers::modelarmor::SanitizeMode::Warn,
                    &crate::formatter::OutputFormat::default(),
                    false,
                )
                .await?;

                return Ok(true);
            }
            if let Some(matches) = matches.subcommand_matches("+agenda") {
                handle_agenda(matches).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}
async fn handle_agenda(matches: &ArgMatches) -> Result<(), GwsError> {
    let cal_scope = "https://www.googleapis.com/auth/calendar.readonly";
    let token = auth::get_token(&[cal_scope])
        .await
        .map_err(|e| GwsError::Auth(format!("Calendar auth failed: {e}")))?;

    let output_format = matches
        .get_one::<String>("format")
        .map(|s| crate::formatter::OutputFormat::from_str(s))
        .unwrap_or(crate::formatter::OutputFormat::Table);

    let client = crate::client::build_client()?;
    let tz_override = matches.get_one::<String>("timezone").map(|s| s.as_str());
    let tz = crate::timezone::resolve_account_timezone(&client, &token, tz_override).await?;

    // Determine time range using the account timezone so that --today and
    // --tomorrow align with the user's Google account day, not the machine.
    let now_in_tz = chrono::Utc::now().with_timezone(&tz);
    let today_start_tz = crate::timezone::start_of_today(tz)?;

    let days: i64 = if matches.get_flag("tomorrow") {
        1
    } else if matches.get_flag("week") {
        7
    } else {
        matches
            .get_one::<String>("days")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(1)
    };

    let (time_min_dt, time_max_dt) = if matches.get_flag("today") {
        // Today: account tz midnight to midnight+1
        let end = today_start_tz + chrono::Duration::days(1);
        (today_start_tz, end)
    } else if matches.get_flag("tomorrow") {
        // Tomorrow: account tz midnight+1 to midnight+2
        let start = today_start_tz + chrono::Duration::days(1);
        let end = today_start_tz + chrono::Duration::days(2);
        (start, end)
    } else {
        // From now, N days ahead
        let end = now_in_tz + chrono::Duration::days(days);
        (now_in_tz, end)
    };

    let time_min = time_min_dt.to_rfc3339();
    let time_max = time_max_dt.to_rfc3339();

    // client already built above for timezone resolution
    let calendar_filter = matches.get_one::<String>("calendar");

    // 1. List all calendars
    let list_url = "https://www.googleapis.com/calendar/v3/users/me/calendarList";
    let list_resp = client
        .get(list_url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to list calendars: {e}")))?;

    if !list_resp.status().is_success() {
        let err = list_resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: 0,
            message: err,
            reason: "calendarList_failed".to_string(),
            enable_url: None,
        });
    }

    let list_json: Value = list_resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse calendar list: {e}")))?;

    let calendars = list_json
        .get("items")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();

    // 2. For each calendar, fetch events concurrently
    use futures_util::stream::{self, StreamExt};

    // Pre-filter calendars and collect owned data to avoid lifetime issues
    struct CalInfo {
        id: String,
        summary: String,
    }
    let filtered_calendars: Vec<CalInfo> = calendars
        .iter()
        .filter_map(|cal| {
            let cal_id = cal.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let cal_summary = cal
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or(cal_id);

            // Apply calendar filter
            if let Some(filter) = calendar_filter {
                if !cal_summary.contains(filter.as_str()) && cal_id != filter.as_str() {
                    return None;
                }
            }

            Some(CalInfo {
                id: cal_id.to_string(),
                summary: cal_summary.to_string(),
            })
        })
        .collect();

    let mut all_events: Vec<Value> = stream::iter(filtered_calendars)
        .map(|cal| {
            let client = &client;
            let token = &token;
            let time_min = &time_min;
            let time_max = &time_max;
            async move {
                let events_url = format!(
                    "https://www.googleapis.com/calendar/v3/calendars/{}/events",
                    crate::validate::encode_path_segment(&cal.id),
                );

                let resp = crate::client::send_with_retry(|| {
                    client
                        .get(&events_url)
                        .query(&[
                            ("timeMin", time_min.as_str()),
                            ("timeMax", time_max.as_str()),
                            ("singleEvents", "true"),
                            ("orderBy", "startTime"),
                            ("maxResults", "50"),
                        ])
                        .bearer_auth(token)
                })
                .await;

                let resp = match resp {
                    Ok(r) if r.status().is_success() => r,
                    _ => return vec![],
                };

                let events_json: Value = match resp.json().await {
                    Ok(v) => v,
                    Err(_) => return vec![],
                };

                let mut events = Vec::new();
                if let Some(items) = events_json.get("items").and_then(|i| i.as_array()) {
                    for event in items {
                        let start = event
                            .get("start")
                            .and_then(|s| s.get("dateTime").or_else(|| s.get("date")))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let end = event
                            .get("end")
                            .and_then(|s| s.get("dateTime").or_else(|| s.get("date")))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let summary = event
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(No title)")
                            .to_string();
                        let location = event
                            .get("location")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        events.push(json!({
                            "start": start,
                            "end": end,
                            "summary": summary,
                            "calendar": cal.summary,
                            "location": location,
                        }));
                    }
                }
                events
            }
        })
        .buffer_unordered(5)
        .flat_map(stream::iter)
        .collect()
        .await;

    // 3. Sort by start time
    all_events.sort_by(|a, b| {
        let a_start = a.get("start").and_then(|v| v.as_str()).unwrap_or("");
        let b_start = b.get("start").and_then(|v| v.as_str()).unwrap_or("");
        a_start.cmp(b_start)
    });

    let output = json!({
        "events": all_events,
        "count": all_events.len(),
        "timeMin": time_min,
        "timeMax": time_max,
    });

    println!(
        "{}",
        crate::formatter::format_value(&output, &output_format)
    );
    Ok(())
}

fn build_insert_request(
    matches: &ArgMatches,
    doc: &crate::discovery::RestDescription,
) -> Result<(String, String, Vec<String>), GwsError> {
    let calendar_id = matches.get_one::<String>("calendar").unwrap();
    let summary = matches.get_one::<String>("summary").unwrap();
    let start = matches.get_one::<String>("start").unwrap();
    let end = matches.get_one::<String>("end").unwrap();
    let location = matches.get_one::<String>("location");
    let description = matches.get_one::<String>("description");
    let attendees_vals = matches.get_many::<String>("attendee");

    // Find method: events.insert checks
    let events_res = doc
        .resources
        .get("events")
        .ok_or_else(|| GwsError::Discovery("Resource 'events' not found".to_string()))?;
    let insert_method = events_res
        .methods
        .get("insert")
        .ok_or_else(|| GwsError::Discovery("Method 'events.insert' not found".to_string()))?;

    // Build body
    let mut body = json!({
        "summary": summary,
        "start": { "dateTime": start },
        "end": { "dateTime": end },
    });

    if let Some(loc) = location {
        body["location"] = json!(loc);
    }
    if let Some(desc) = description {
        body["description"] = json!(desc);
    }

    if let Some(atts) = attendees_vals {
        let attendees_list: Vec<_> = atts.map(|email| json!({ "email": email })).collect();
        body["attendees"] = json!(attendees_list);
    }

    let mut params = json!({
        "calendarId": calendar_id
    });

    if matches.get_flag("meet") {
        let namespace = uuid::Uuid::NAMESPACE_DNS;

        let mut attendees: Vec<_> = matches
            .get_many::<String>("attendee")
            .map(|vals| vals.cloned().collect())
            .unwrap_or_default();
        attendees.sort();

        let seed_payload = {
            let mut map = serde_json::Map::new();
            map.insert("v".to_string(), json!(1));
            map.insert("summary".to_string(), json!(summary));
            map.insert("start".to_string(), json!(start));
            map.insert("end".to_string(), json!(end));
            if let Some(loc) = location {
                map.insert("location".to_string(), json!(loc));
            }
            if let Some(desc) = description {
                map.insert("description".to_string(), json!(desc));
            }
            if !attendees.is_empty() {
                let attendees_list_for_seed: Vec<_> = attendees
                    .iter()
                    .map(|email| json!({ "email": email }))
                    .collect();
                map.insert("attendees".to_string(), json!(attendees_list_for_seed));
            }
            serde_json::Value::Object(map)
        };

        let seed_data = serde_json::to_vec(&seed_payload).map_err(|e| {
            GwsError::Other(anyhow::anyhow!(
                "Failed to serialize seed payload for idempotency key: {e}"
            ))
        })?;
        let request_id = uuid::Uuid::new_v5(&namespace, &seed_data).to_string();

        body["conferenceData"] = json!({
            "createRequest": {
                "requestId": request_id,
                "conferenceSolutionKey": { "type": "hangoutsMeet" }
            }
        });
        params["conferenceDataVersion"] = json!(1);
    }
    let body_str = body.to_string();
    let scopes: Vec<String> = insert_method.scopes.iter().map(|s| s.to_string()).collect();

    // events.insert requires 'calendarId' path parameter
    let params_str = params.to_string();

    Ok((params_str, body_str, scopes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mock_doc() -> crate::discovery::RestDescription {
        let mut doc = crate::discovery::RestDescription::default();
        let mut events_res = crate::discovery::RestResource::default();
        let mut insert_method = crate::discovery::RestMethod::default();
        insert_method.scopes.push("https://scope".to_string());
        events_res
            .methods
            .insert("insert".to_string(), insert_method);
        doc.resources.insert("events".to_string(), events_res);
        doc
    }

    fn make_matches_insert(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(
                Arg::new("calendar")
                    .long("calendar")
                    .default_value("primary"),
            )
            .arg(Arg::new("summary").long("summary").required(true))
            .arg(Arg::new("start").long("start").required(true))
            .arg(Arg::new("end").long("end").required(true))
            .arg(Arg::new("location").long("location"))
            .arg(Arg::new("description").long("description"))
            .arg(
                Arg::new("attendee")
                    .long("attendee")
                    .action(ArgAction::Append),
            )
            .arg(Arg::new("meet").long("meet").action(ArgAction::SetTrue));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_build_insert_request() {
        let doc = make_mock_doc();
        let matches = make_matches_insert(&[
            "test",
            "--summary",
            "Meeting",
            "--start",
            "2024-01-01T10:00:00Z",
            "--end",
            "2024-01-01T11:00:00Z",
        ]);
        let (params, body, scopes) = build_insert_request(&matches, &doc).unwrap();

        assert!(params.contains("primary"));
        assert!(body.contains("Meeting"));
        assert!(body.contains("2024-01-01T10:00:00Z"));
        assert_eq!(scopes[0], "https://scope");
    }

    #[test]
    fn test_build_insert_request_with_meet() {
        let doc = make_mock_doc();
        let matches = make_matches_insert(&[
            "test",
            "--summary",
            "Meeting",
            "--start",
            "2024-01-01T10:00:00Z",
            "--end",
            "2024-01-01T11:00:00Z",
            "--meet",
        ]);
        let (params, body, _) = build_insert_request(&matches, &doc).unwrap();

        let params_json: serde_json::Value = serde_json::from_str(&params).unwrap();
        assert_eq!(params_json["conferenceDataVersion"], 1);

        let body_json: serde_json::Value = serde_json::from_str(&body).unwrap();
        let create_req = &body_json["conferenceData"]["createRequest"];
        assert_eq!(create_req["conferenceSolutionKey"]["type"], "hangoutsMeet");
        assert!(uuid::Uuid::parse_str(create_req["requestId"].as_str().unwrap()).is_ok());
    }

    #[test]
    fn test_build_insert_request_with_meet_is_idempotent() {
        let doc = make_mock_doc();
        let args = &[
            "test",
            "--summary",
            "Idempotent Meeting",
            "--start",
            "2024-01-01T10:00:00Z",
            "--end",
            "2024-01-01T11:00:00Z",
            "--meet",
        ];
        let matches1 = make_matches_insert(args);
        let (_, body1, _) = build_insert_request(&matches1, &doc).unwrap();

        let matches2 = make_matches_insert(args);
        let (_, body2, _) = build_insert_request(&matches2, &doc).unwrap();

        let b1: serde_json::Value = serde_json::from_str(&body1).unwrap();
        let b2: serde_json::Value = serde_json::from_str(&body2).unwrap();

        assert_eq!(
            b1["conferenceData"]["createRequest"]["requestId"],
            b2["conferenceData"]["createRequest"]["requestId"],
            "requestId should be deterministic for the same event details"
        );
    }

    #[test]
    fn test_build_insert_request_with_meet_idempotency_robust() {
        let doc = make_mock_doc();

        // Base case
        let args_base = &[
            "test",
            "--summary",
            "S",
            "--start",
            "2024-01-01T10:00:00Z",
            "--end",
            "2024-01-01T11:00:00Z",
            "--meet",
            "--attendee",
            "a@b.com",
            "--attendee",
            "c@d.com",
        ];
        let (_, body_base, _) =
            build_insert_request(&make_matches_insert(args_base), &doc).unwrap();
        let b_base: serde_json::Value = serde_json::from_str(&body_base).unwrap();
        let id_base = b_base["conferenceData"]["createRequest"]["requestId"]
            .as_str()
            .unwrap();

        // Same but different attendee order
        let args_reordered = &[
            "test",
            "--summary",
            "S",
            "--start",
            "2024-01-01T10:00:00Z",
            "--end",
            "2024-01-01T11:00:00Z",
            "--meet",
            "--attendee",
            "c@d.com",
            "--attendee",
            "a@b.com",
        ];
        let (_, body_reordered, _) =
            build_insert_request(&make_matches_insert(args_reordered), &doc).unwrap();
        let b_reordered: serde_json::Value = serde_json::from_str(&body_reordered).unwrap();
        let id_reordered = b_reordered["conferenceData"]["createRequest"]["requestId"]
            .as_str()
            .unwrap();

        assert_eq!(
            id_base, id_reordered,
            "Attendee order should not change requestId"
        );

        // Different summary -> different ID
        let args_diff = &[
            "test",
            "--summary",
            "Diff",
            "--start",
            "2024-01-01T10:00:00Z",
            "--end",
            "2024-01-01T11:00:00Z",
            "--meet",
            "--attendee",
            "a@b.com",
            "--attendee",
            "c@d.com",
        ];
        let (_, body_diff, _) =
            build_insert_request(&make_matches_insert(args_diff), &doc).unwrap();
        let b_diff: serde_json::Value = serde_json::from_str(&body_diff).unwrap();
        let id_diff = b_diff["conferenceData"]["createRequest"]["requestId"]
            .as_str()
            .unwrap();

        assert_ne!(
            id_base, id_diff,
            "Different summary should produce different requestId"
        );
    }

    #[test]
    fn test_build_insert_request_with_optional_fields() {
        let doc = make_mock_doc();
        let matches = make_matches_insert(&[
            "test",
            "--summary",
            "Meeting",
            "--start",
            "2024-01-01T10:00:00Z",
            "--end",
            "2024-01-01T11:00:00Z",
            "--location",
            "Room 1",
            "--description",
            "Discuss stuff",
            "--attendee",
            "a@b.com",
            "--attendee",
            "c@d.com",
        ]);
        let (_, body, _) = build_insert_request(&matches, &doc).unwrap();

        assert!(body.contains("Room 1"));
        assert!(body.contains("Discuss stuff"));
        assert!(body.contains("a@b.com"));
        assert!(body.contains("c@d.com"));
    }

    /// Verify that agenda day boundaries use a specific timezone, not UTC.
    #[test]
    fn agenda_day_boundaries_use_account_timezone() {
        use chrono::{NaiveTime, TimeZone, Utc};

        // Simulate using a known account timezone (America/Denver = UTC-7 / UTC-6 DST)
        let tz = chrono_tz::America::Denver;
        let now_in_tz = Utc::now().with_timezone(&tz);
        let today_start = now_in_tz
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let today_start_tz = tz
            .from_local_datetime(&today_start)
            .earliest()
            .expect("midnight should resolve");

        let today_rfc = today_start_tz.to_rfc3339();
        let tomorrow_start = today_start_tz + chrono::Duration::days(1);
        let tomorrow_rfc = tomorrow_start.to_rfc3339();

        // The Denver offset should appear in the RFC3339 string (-07:00 or -06:00 for DST).
        // Crucially, it should NOT be +00:00 (UTC).
        assert!(
            today_rfc.contains("-07:00") || today_rfc.contains("-06:00"),
            "today boundary should carry Denver offset, got {today_rfc}"
        );
        assert!(
            tomorrow_rfc.contains("-07:00") || tomorrow_rfc.contains("-06:00"),
            "tomorrow boundary should carry Denver offset, got {tomorrow_rfc}"
        );
    }
}

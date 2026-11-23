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
        cmd = cmd.subcommand(
            Command::new("+free")
                .about("[Helper] Find free time slots across your calendars")
                .arg(
                    Arg::new("days")
                        .long("days")
                        .help("Days to search, starting today (default 3, max 60)")
                        .value_parser(clap::value_parser!(i64).range(1..=60))
                        .default_value("3")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("minutes")
                        .long("minutes")
                        .help("Shortest free slot to show, in minutes (default 30)")
                        .value_parser(clap::value_parser!(i64).range(5..=1440))
                        .default_value("30")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("between")
                        .long("between")
                        .help("Working hours each day, HH:MM-HH:MM (default 09:00-18:00)")
                        .default_value("09:00-18:00")
                        .value_name("RANGE"),
                )
                .arg(
                    Arg::new("weekdays")
                        .long("weekdays")
                        .help("Skip Saturdays and Sundays")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("calendar")
                        .long("calendar")
                        .help("Only check calendars whose name or ID matches (repeatable)")
                        .action(ArgAction::Append)
                        .value_name("NAME"),
                )
                .arg(
                    Arg::new("count-all-day")
                        .long("count-all-day")
                        .help("Let busy all-day events block whole days (by default only out-of-office ones do)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("all-calendars")
                        .long("all-calendars")
                        .help("Also count calendars you only subscribe to (default: calendars you own or edit)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("timezone")
                        .long("timezone")
                        .alias("tz")
                        .help("IANA timezone for --between and the output. Defaults to your Google account timezone.")
                        .value_name("TZ"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws calendar +free
  gws calendar +free --days 5 --minutes 60
  gws calendar +free --between 10:00-16:30 --weekdays
  gws calendar +free --calendar Work --format json

TIPS:
  Read-only. Timed events block time unless marked 'free' or declined.
  All-day events (postings, trips, reminders) don't block time unless they
  are out-of-office or you pass --count-all-day. Calendars that share only
  free/busy are read through the freeBusy API. Slots that have already
  started today begin at the next quarter hour.",
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
            if let Some(matches) = matches.subcommand_matches("+free") {
                handle_free(matches).await?;
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

                events_json
                    .get("items")
                    .and_then(|i| i.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .map(|event| event_entry(event, &cal.summary))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
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

    print!(
        "{}",
        render_agenda(&all_events, &time_min, &time_max, &output_format)
    );
    Ok(())
}

/// One agenda entry. `meet` is the event's Google Meet link, if any.
pub(crate) fn event_entry(event: &Value, calendar: &str) -> Value {
    let when = |key: &str| {
        event
            .get(key)
            .and_then(|s| s.get("dateTime").or_else(|| s.get("date")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let text = |key: &str, default: &str| {
        event
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    };
    json!({
        "start": when("start"),
        "end": when("end"),
        "summary": text("summary", "(No title)"),
        "calendar": calendar,
        "location": text("location", ""),
        "meet": meet_link(event).unwrap_or_default(),
    })
}

/// The Google Meet (or other video) link of an event: `hangoutLink`, else the
/// first video entry point of its conference data.
pub(crate) fn meet_link(event: &Value) -> Option<String> {
    if let Some(link) = event.get("hangoutLink").and_then(|v| v.as_str()) {
        if !link.is_empty() {
            return Some(link.to_string());
        }
    }
    event
        .pointer("/conferenceData/entryPoints")
        .and_then(|e| e.as_array())?
        .iter()
        .find(|ep| ep.get("entryPointType").and_then(|t| t.as_str()) == Some("video"))
        .and_then(|ep| ep.get("uri").and_then(|u| u.as_str()))
        .map(str::to_string)
}

/// Agenda output. Table and CSV keep a readable column order; JSON and YAML
/// keep the `{events, count, timeMin, timeMax}` shape.
pub(crate) fn render_agenda(
    events: &[Value],
    time_min: &str,
    time_max: &str,
    format: &crate::formatter::OutputFormat,
) -> String {
    use crate::formatter::OutputFormat;
    match format {
        OutputFormat::Table | OutputFormat::Csv => {
            let has_profile = events.iter().any(|e| e.get("profile").is_some());
            let mut keys: Vec<&str> = vec!["start", "end", "summary", "calendar"];
            if has_profile {
                keys.push("profile");
            }
            keys.extend(["meet", "location"]);
            let columns: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
            let rows: Vec<Vec<Value>> = events
                .iter()
                .map(|e| {
                    keys.iter()
                        .map(|k| e.get(*k).cloned().unwrap_or(Value::Null))
                        .collect()
                })
                .collect();
            let rendered = crate::formatter::render_report(
                &columns,
                &rows,
                &json!({}),
                None,
                &format!("No events between {time_min} and {time_max}."),
                format,
            );
            if let Some(n) = rendered.notice {
                eprintln!("{n}");
            }
            rendered.stdout
        }
        _ => {
            let output = json!({
                "events": events,
                "count": events.len(),
                "timeMin": time_min,
                "timeMax": time_max,
            });
            let mut out = crate::formatter::format_value(&output, format);
            out.push('\n');
            out
        }
    }
}

// ── +free ────────────────────────────────────────────────────────────

/// Daily working window parsed from `--between`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DailyWindow {
    pub start: chrono::NaiveTime,
    pub end: chrono::NaiveTime,
}

fn parse_clock(s: &str) -> Option<chrono::NaiveTime> {
    let s = s.trim();
    let (h, m) = match s.split_once(':') {
        Some((h, m)) => (h, m),
        None => (s, "0"),
    };
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if h == 24 && m == 0 {
        return chrono::NaiveTime::from_hms_opt(23, 59, 59);
    }
    chrono::NaiveTime::from_hms_opt(h, m, 0)
}

/// Parses `HH:MM-HH:MM` (also `9-18`, `9:30-17`). The end must be after the start.
pub(crate) fn parse_between(raw: &str) -> Result<DailyWindow, GwsError> {
    let bad = || {
        GwsError::Validation(format!(
            "--between must look like 09:00-18:00 (24-hour clock), got '{raw}'"
        ))
    };
    let (a, b) = raw.split_once('-').ok_or_else(bad)?;
    let start = parse_clock(a).ok_or_else(bad)?;
    let end = parse_clock(b).ok_or_else(bad)?;
    if end <= start {
        return Err(GwsError::Validation(format!(
            "--between end ({b}) must be after its start ({a})"
        )));
    }
    Ok(DailyWindow { start, end })
}

/// A busy or free interval in UTC.
pub(crate) type Interval = (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>);

/// Rounds up to the next quarter hour (unchanged if already on one).
pub(crate) fn next_quarter_hour(t: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    let secs = t.timestamp();
    let q = 15 * 60;
    let rounded = (secs + q - 1).div_euclid(q) * q;
    let base = chrono::DateTime::from_timestamp(rounded, 0).unwrap_or(t);
    if t.timestamp_subsec_nanos() > 0 && rounded == secs {
        base + chrono::Duration::minutes(15)
    } else {
        base
    }
}

/// The working windows (in UTC) for `days` days starting on `today`.
pub(crate) fn working_windows<Tz: chrono::TimeZone>(
    tz: &Tz,
    today: chrono::NaiveDate,
    days: i64,
    window: DailyWindow,
    weekdays_only: bool,
) -> Vec<Interval> {
    use chrono::Datelike;
    (0..days)
        .filter_map(|i| {
            let day = today + chrono::Duration::days(i);
            if weekdays_only && matches!(day.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
            {
                return None;
            }
            let start = tz
                .from_local_datetime(&day.and_time(window.start))
                .earliest()?;
            let end = tz.from_local_datetime(&day.and_time(window.end)).latest()?;
            Some((
                start.with_timezone(&chrono::Utc),
                end.with_timezone(&chrono::Utc),
            ))
        })
        .collect()
}

/// Sorts and merges overlapping or touching intervals.
pub(crate) fn merge_intervals(mut busy: Vec<Interval>) -> Vec<Interval> {
    busy.retain(|(s, e)| e > s);
    busy.sort_by_key(|(s, _)| *s);
    let mut merged: Vec<Interval> = Vec::new();
    for (s, e) in busy {
        match merged.last_mut() {
            Some(last) if s <= last.1 => {
                if e > last.1 {
                    last.1 = e;
                }
            }
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// Free gaps of at least `min_minutes` inside each window, not before `now`.
pub(crate) fn free_slots(
    windows: &[Interval],
    busy: Vec<Interval>,
    now: chrono::DateTime<chrono::Utc>,
    min_minutes: i64,
) -> Vec<Interval> {
    let busy = merge_intervals(busy);
    let earliest = next_quarter_hour(now);
    let min = chrono::Duration::minutes(min_minutes);
    let mut out = Vec::new();
    for &(w_start, w_end) in windows {
        let mut cursor = w_start.max(earliest);
        if cursor >= w_end {
            continue;
        }
        for &(b_start, b_end) in busy.iter().filter(|(s, e)| *e > w_start && *s < w_end) {
            if b_start > cursor && b_start - cursor >= min {
                out.push((cursor, b_start.min(w_end)));
            }
            if b_end > cursor {
                cursor = b_end;
            }
            if cursor >= w_end {
                break;
            }
        }
        if cursor < w_end && w_end - cursor >= min {
            out.push((cursor, w_end));
        }
    }
    out
}

/// Parses the busy intervals of a freeBusy response, plus per-calendar errors.
pub(crate) fn parse_freebusy(resp: &Value) -> (Vec<Interval>, Vec<String>) {
    let mut busy = Vec::new();
    let mut errors = Vec::new();
    if let Some(cals) = resp.get("calendars").and_then(|c| c.as_object()) {
        for (id, cal) in cals {
            if let Some(errs) = cal.get("errors").and_then(|e| e.as_array()) {
                for e in errs {
                    let reason = e.get("reason").and_then(|r| r.as_str()).unwrap_or("error");
                    errors.push(format!("{id}: {reason}"));
                }
            }
            for b in cal
                .get("busy")
                .and_then(|b| b.as_array())
                .into_iter()
                .flatten()
            {
                let parse = |k: &str| {
                    b.get(k)
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|t| t.with_timezone(&chrono::Utc))
                };
                if let (Some(s), Some(e)) = (parse("start"), parse("end")) {
                    busy.push((s, e));
                }
            }
        }
    }
    (busy, errors)
}

/// Picks the calendars whose busy time counts: by default those you own or
/// can edit; with `include_all`, every listed calendar. `filters` narrow by
/// name or ID.
pub(crate) fn select_calendars(
    calendar_list: &Value,
    filters: &[String],
    include_all: bool,
) -> Vec<(String, String)> {
    calendar_list
        .get("items")
        .and_then(|i| i.as_array())
        .into_iter()
        .flatten()
        .filter_map(|cal| {
            let id = cal.get("id")?.as_str()?.to_string();
            let name = cal
                .get("summaryOverride")
                .or_else(|| cal.get("summary"))
                .and_then(|v| v.as_str())
                .unwrap_or(&id)
                .to_string();
            let role = cal.get("accessRole").and_then(|v| v.as_str()).unwrap_or("");
            let primary = cal
                .get("primary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let own = primary || role == "owner" || role == "writer";
            if !include_all && !own {
                return None;
            }
            if !filters.is_empty()
                && !filters
                    .iter()
                    .any(|f| f == &id || name.to_lowercase().contains(&f.to_lowercase()))
            {
                return None;
            }
            Some((id, name))
        })
        .collect()
}

fn duration_text(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Rows for the slots: day, start and end in `tz`, and length.
pub(crate) fn slot_rows<Tz: chrono::TimeZone>(
    slots: &[Interval],
    tz: &Tz,
    human: bool,
) -> Vec<Vec<Value>>
where
    Tz::Offset: std::fmt::Display,
{
    slots
        .iter()
        .map(|(s, e)| {
            let ls = s.with_timezone(tz);
            let le = e.with_timezone(tz);
            let minutes = (*e - *s).num_minutes();
            if human {
                vec![
                    json!(ls.format("%a %d %b").to_string()),
                    json!(ls.format("%H:%M").to_string()),
                    json!(le.format("%H:%M").to_string()),
                    json!(duration_text(minutes)),
                ]
            } else {
                vec![
                    json!(ls.format("%Y-%m-%d").to_string()),
                    json!(ls.to_rfc3339()),
                    json!(le.to_rfc3339()),
                    json!(minutes),
                ]
            }
        })
        .collect()
}

/// The busy interval an event contributes, or None if it doesn't block time:
/// cancelled, marked free (transparent), declined by you, or an all-day
/// event (unless out-of-office or `count_all_day`).
pub(crate) fn event_busy_interval<Tz: chrono::TimeZone>(
    event: &Value,
    tz: &Tz,
    count_all_day: bool,
) -> Option<Interval> {
    if event.get("status").and_then(|v| v.as_str()) == Some("cancelled") {
        return None;
    }
    if event.get("transparency").and_then(|v| v.as_str()) == Some("transparent") {
        return None;
    }
    let declined = event
        .get("attendees")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .any(|a| {
            a.get("self").and_then(|v| v.as_bool()) == Some(true)
                && a.get("responseStatus").and_then(|v| v.as_str()) == Some("declined")
        });
    if declined {
        return None;
    }
    let out_of_office = event.get("eventType").and_then(|v| v.as_str()) == Some("outOfOffice");
    let timed = |k: &str| {
        event
            .pointer(&format!("/{k}/dateTime"))
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&chrono::Utc))
    };
    if let (Some(s), Some(e)) = (timed("start"), timed("end")) {
        return Some((s, e));
    }
    if !(count_all_day || out_of_office) {
        return None;
    }
    let date = |k: &str| {
        event
            .pointer(&format!("/{k}/date"))
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .and_then(|d| {
                tz.from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
                    .earliest()
                    .map(|t| t.with_timezone(&chrono::Utc))
            })
    };
    Some((date("start")?, date("end")?))
}

/// Busy intervals of one calendar from its events. `Ok(None)` means the
/// calendar shares only free/busy (no event access), so the caller should
/// fall back to the freeBusy API.
async fn busy_from_events<Tz: chrono::TimeZone>(
    client: &reqwest::Client,
    token: &str,
    calendar_id: &str,
    time_min: &str,
    time_max: &str,
    tz: &Tz,
    count_all_day: bool,
) -> Result<Option<Vec<Interval>>, GwsError> {
    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/{}/events",
        crate::validate::encode_path_segment(calendar_id)
    );
    let mut busy = Vec::new();
    let mut page: Option<String> = None;
    // 2500 events a page; 20 pages is far more than any 60-day window holds.
    for _ in 0..20 {
        let result = crate::executor::fetch_json(
            || {
                let mut q: Vec<(&str, String)> = vec![
                    ("timeMin", time_min.to_string()),
                    ("timeMax", time_max.to_string()),
                    ("singleEvents", "true".to_string()),
                    ("maxResults", "2500".to_string()),
                    (
                        "fields",
                        "items(status,transparency,eventType,start,end,attendees(self,responseStatus)),nextPageToken"
                            .to_string(),
                    ),
                ];
                if let Some(p) = &page {
                    q.push(("pageToken", p.clone()));
                }
                client.get(&url).query(&q).bearer_auth(token)
            },
            "Calendar events list",
        )
        .await;
        let resp = match result {
            Ok(r) => r,
            Err(GwsError::Api {
                code: 403 | 404, ..
            }) => return Ok(None),
            Err(e) => return Err(e),
        };
        busy.extend(
            resp.get("items")
                .and_then(|i| i.as_array())
                .into_iter()
                .flatten()
                .filter_map(|e| event_busy_interval(e, tz, count_all_day)),
        );
        page = resp
            .get("nextPageToken")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if page.is_none() {
            break;
        }
    }
    Ok(Some(busy))
}

/// Busy intervals for the given calendars from the freeBusy API, in batches
/// of 50 (the API's per-request limit).
async fn fetch_busy(
    client: &reqwest::Client,
    token: &str,
    calendars: &[(String, String)],
    time_min: &str,
    time_max: &str,
) -> Result<(Vec<Interval>, Vec<String>), GwsError> {
    let mut busy = Vec::new();
    let mut errors = Vec::new();
    for batch in calendars.chunks(50) {
        let body = json!({
            "timeMin": time_min,
            "timeMax": time_max,
            "items": batch.iter().map(|(id, _)| json!({"id": id})).collect::<Vec<_>>(),
        });
        let resp = crate::executor::fetch_json(
            || {
                client
                    .post("https://www.googleapis.com/calendar/v3/freeBusy")
                    .bearer_auth(token)
                    .json(&body)
            },
            "Calendar freeBusy",
        )
        .await?;
        let (b, e) = parse_freebusy(&resp);
        busy.extend(b);
        errors.extend(e);
    }
    Ok((busy, errors))
}

async fn handle_free(matches: &ArgMatches) -> Result<(), GwsError> {
    let window = parse_between(
        matches
            .get_one::<String>("between")
            .map(String::as_str)
            .unwrap_or("09:00-18:00"),
    )?;
    let days = *matches.get_one::<i64>("days").unwrap_or(&3);
    let min_minutes = *matches.get_one::<i64>("minutes").unwrap_or(&30);
    let filters: Vec<String> = matches
        .get_many::<String>("calendar")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let format = matches
        .get_one::<String>("format")
        .map(|s| crate::formatter::OutputFormat::from_str(s))
        .unwrap_or(crate::formatter::OutputFormat::Table);

    let token = auth::get_token(&["https://www.googleapis.com/auth/calendar.readonly"])
        .await
        .map_err(|e| GwsError::Auth(format!("Calendar auth failed: {e}")))?;
    let client = crate::client::build_client()?;
    let tz_override = matches.get_one::<String>("timezone").map(|s| s.as_str());
    let tz = crate::timezone::resolve_account_timezone(&client, &token, tz_override).await?;

    let now = chrono::Utc::now();
    let today = now.with_timezone(&tz).date_naive();
    let windows = working_windows(&tz, today, days, window, matches.get_flag("weekdays"));
    let (Some(first), Some(last)) = (windows.first(), windows.last()) else {
        println!("No working days in the next {days} days.");
        return Ok(());
    };
    let time_min = first.0.to_rfc3339();
    let time_max = last.1.to_rfc3339();

    let list = crate::executor::fetch_json(
        || {
            client
                .get("https://www.googleapis.com/calendar/v3/users/me/calendarList")
                .query(&[("maxResults", "250")])
                .bearer_auth(&token)
        },
        "Calendar list",
    )
    .await?;
    let calendars = select_calendars(&list, &filters, matches.get_flag("all-calendars"));
    if calendars.is_empty() {
        return Err(GwsError::Validation(
            "No calendars matched; check --calendar (see: gws calendar calendarList list)".into(),
        ));
    }
    let count_all_day = matches.get_flag("count-all-day");
    let mut busy: Vec<Interval> = Vec::new();
    let mut freebusy_only: Vec<(String, String)> = Vec::new();
    {
        use futures_util::stream::{self, StreamExt};
        let results: Vec<_> = stream::iter(calendars.iter().cloned())
            .map(|(id, name)| {
                let (client, token, time_min, time_max, tz) =
                    (&client, &token, &time_min, &time_max, &tz);
                async move {
                    let r =
                        busy_from_events(client, token, &id, time_min, time_max, tz, count_all_day)
                            .await;
                    ((id, name), r)
                }
            })
            .buffer_unordered(5)
            .collect()
            .await;
        for (cal, r) in results {
            match r? {
                Some(b) => busy.extend(b),
                None => freebusy_only.push(cal),
            }
        }
    }
    let mut errors = Vec::new();
    if !freebusy_only.is_empty() {
        let (b, e) = fetch_busy(&client, &token, &freebusy_only, &time_min, &time_max).await?;
        busy.extend(b);
        errors = e;
    }
    for e in &errors {
        eprintln!("warning: could not read busy times for {e}");
    }

    let slots = free_slots(&windows, busy, now, min_minutes);
    let human = matches!(
        format,
        crate::formatter::OutputFormat::Table | crate::formatter::OutputFormat::Csv
    );
    let columns: Vec<String> = if human {
        ["day", "start", "end", "length"]
    } else {
        ["date", "start", "end", "minutes"]
    }
    .iter()
    .map(|s| s.to_string())
    .collect();
    let rows = slot_rows(&slots, &tz, human);
    let between = format!(
        "{}-{}",
        window.start.format("%H:%M"),
        window.end.format("%H:%M")
    );
    let title = format!(
        "Free {min_minutes}+ min, {between} {tz}, next {days} day(s) · {} calendar(s) checked",
        calendars.len()
    );
    let meta = json!({
        "timeZone": tz.to_string(),
        "between": between,
        "minMinutes": min_minutes,
        "timeMin": time_min,
        "timeMax": time_max,
        "calendars": calendars.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
        "countAllDay": count_all_day,
        "errors": errors,
    });
    let rendered = crate::formatter::render_report(
        &columns,
        &rows,
        &meta,
        Some(&title),
        &format!(
            "No free slots of {min_minutes}+ minutes between {between} ({tz}) in the next {days} day(s)."
        ),
        &format,
    );
    print!("{}", rendered.stdout);
    if let Some(n) = rendered.notice {
        eprintln!("{n}");
    }
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
    // ── +agenda: Meet links and ordered output ──────────────────────

    #[test]
    fn meet_link_prefers_hangout_link_then_video_entry_point() {
        assert_eq!(
            meet_link(&json!({"hangoutLink": "https://meet.google.com/abc-defg-hij"})).as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        let ev = json!({"conferenceData": {"entryPoints": [
            {"entryPointType": "phone", "uri": "tel:+1"},
            {"entryPointType": "video", "uri": "https://zoom.us/j/1"}
        ]}});
        assert_eq!(meet_link(&ev).as_deref(), Some("https://zoom.us/j/1"));
        assert_eq!(meet_link(&json!({"hangoutLink": ""})), None);
        assert_eq!(meet_link(&json!({})), None);
    }

    #[test]
    fn event_entry_fields() {
        let e = event_entry(
            &json!({
                "start": {"dateTime": "2026-11-24T10:00:00+01:00"},
                "end": {"date": "2026-11-25"},
                "hangoutLink": "https://meet.google.com/x"
            }),
            "Work",
        );
        assert_eq!(e["start"], "2026-11-24T10:00:00+01:00");
        assert_eq!(e["end"], "2026-11-25");
        assert_eq!(e["summary"], "(No title)");
        assert_eq!(e["calendar"], "Work");
        assert_eq!(e["location"], "");
        assert_eq!(e["meet"], "https://meet.google.com/x");
    }

    fn agenda_events() -> Vec<Value> {
        vec![
            json!({"start": "2026-11-24T10:00:00+01:00", "end": "2026-11-24T11:00:00+01:00",
                    "summary": "Standup", "calendar": "Work", "location": "",
                    "meet": "https://meet.google.com/x"}),
        ]
    }

    #[test]
    fn render_agenda_table_orders_columns() {
        let out = render_agenda(
            &agenda_events(),
            "a",
            "b",
            &crate::formatter::OutputFormat::Table,
        );
        let header = out.lines().next().unwrap();
        let pos = |c: &str| header.find(c).unwrap();
        assert!(pos("start") < pos("end") && pos("end") < pos("summary"));
        assert!(pos("calendar") < pos("meet") && pos("meet") < pos("location"));
        assert!(!header.contains("profile"));
        assert!(out.contains("https://meet.google.com/x"));
    }

    #[test]
    fn render_agenda_shows_profile_column_when_present() {
        let mut ev = agenda_events();
        ev[0]["profile"] = json!("bachs");
        let out = render_agenda(&ev, "a", "b", &crate::formatter::OutputFormat::Csv);
        assert_eq!(
            out.lines().next().unwrap(),
            "start,end,summary,calendar,profile,meet,location"
        );
        assert!(out.lines().nth(1).unwrap().contains(",bachs,"));
    }

    #[test]
    fn render_agenda_json_keeps_shape_and_empty_table_says_so() {
        let out = render_agenda(
            &agenda_events(),
            "a",
            "b",
            &crate::formatter::OutputFormat::Json,
        );
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["count"], 1);
        assert_eq!(v["timeMin"], "a");
        assert_eq!(v["events"][0]["meet"], "https://meet.google.com/x");
        let empty = render_agenda(&[], "a", "b", &crate::formatter::OutputFormat::Table);
        assert_eq!(empty, "No events between a and b.\n");
    }

    // ── +free ───────────────────────────────────────────────────────

    fn utc(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn wat() -> chrono_tz::Tz {
        chrono_tz::Africa::Lagos
    }

    #[test]
    fn parse_between_forms() {
        let w = parse_between("09:00-18:00").unwrap();
        assert_eq!(w.start.format("%H:%M").to_string(), "09:00");
        assert_eq!(w.end.format("%H:%M").to_string(), "18:00");
        assert_eq!(
            parse_between("9-17:30")
                .unwrap()
                .end
                .format("%H:%M")
                .to_string(),
            "17:30"
        );
        assert!(parse_between("0-24").is_ok());
        assert!(parse_between("18:00-09:00").is_err());
        assert!(parse_between("09:00").is_err());
        assert!(parse_between("25:00-26:00").is_err());
        assert!(parse_between("9:60-10").is_err());
    }

    #[test]
    fn quarter_hour_rounding() {
        assert_eq!(
            next_quarter_hour(utc("2026-11-23T10:00:00Z")),
            utc("2026-11-23T10:00:00Z")
        );
        assert_eq!(
            next_quarter_hour(utc("2026-11-23T10:01:00Z")),
            utc("2026-11-23T10:15:00Z")
        );
        assert_eq!(
            next_quarter_hour(utc("2026-11-23T10:59:59Z")),
            utc("2026-11-23T11:00:00Z")
        );
        assert_eq!(
            next_quarter_hour(utc("2026-11-23T10:00:00.5Z")),
            utc("2026-11-23T10:15:00Z")
        );
    }

    #[test]
    fn windows_follow_timezone_and_weekdays() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 27).unwrap(); // Friday
        let w = parse_between("09:00-18:00").unwrap();
        let all = working_windows(&wat(), day, 3, w, false);
        assert_eq!(all.len(), 3);
        assert_eq!(
            all[0],
            (utc("2026-11-27T08:00:00Z"), utc("2026-11-27T17:00:00Z"))
        );
        let wd = working_windows(&wat(), day, 4, w, true);
        assert_eq!(wd.len(), 2); // Fri and Mon
        assert_eq!(wd[1].0, utc("2026-11-30T08:00:00Z"));
    }

    #[test]
    fn windows_across_dst() {
        // London moves to BST on 2026-03-29.
        let tz = chrono_tz::Europe::London;
        let day = chrono::NaiveDate::from_ymd_opt(2026, 3, 28).unwrap();
        let w = parse_between("09:00-17:00").unwrap();
        let ws = working_windows(&tz, day, 2, w, false);
        assert_eq!(ws[0].0, utc("2026-03-28T09:00:00Z"));
        assert_eq!(ws[1].0, utc("2026-03-29T08:00:00Z"));
    }

    #[test]
    fn merge_overlapping_and_touching() {
        let m = merge_intervals(vec![
            (utc("2026-11-24T11:00:00Z"), utc("2026-11-24T12:00:00Z")),
            (utc("2026-11-24T09:00:00Z"), utc("2026-11-24T10:00:00Z")),
            (utc("2026-11-24T10:00:00Z"), utc("2026-11-24T10:30:00Z")),
            (utc("2026-11-24T09:15:00Z"), utc("2026-11-24T09:45:00Z")),
            (utc("2026-11-24T13:00:00Z"), utc("2026-11-24T13:00:00Z")),
        ]);
        assert_eq!(
            m,
            vec![
                (utc("2026-11-24T09:00:00Z"), utc("2026-11-24T10:30:00Z")),
                (utc("2026-11-24T11:00:00Z"), utc("2026-11-24T12:00:00Z")),
            ]
        );
    }

    #[test]
    fn free_slots_between_busy_blocks() {
        let windows = vec![(utc("2026-11-24T08:00:00Z"), utc("2026-11-24T17:00:00Z"))];
        let busy = vec![
            (utc("2026-11-24T07:00:00Z"), utc("2026-11-24T08:30:00Z")), // straddles start
            (utc("2026-11-24T09:00:00Z"), utc("2026-11-24T09:20:00Z")), // 30-min gap before
            (utc("2026-11-24T09:40:00Z"), utc("2026-11-24T10:00:00Z")), // 20-min gap: too short
            (utc("2026-11-24T16:45:00Z"), utc("2026-11-24T18:00:00Z")), // straddles end
        ];
        let now = utc("2026-11-20T00:00:00Z");
        let slots = free_slots(&windows, busy, now, 30);
        assert_eq!(
            slots,
            vec![
                (utc("2026-11-24T08:30:00Z"), utc("2026-11-24T09:00:00Z")),
                (utc("2026-11-24T10:00:00Z"), utc("2026-11-24T16:45:00Z")),
            ]
        );
    }

    #[test]
    fn free_slots_skip_the_past_and_full_days() {
        let windows = vec![
            (utc("2026-11-23T08:00:00Z"), utc("2026-11-23T17:00:00Z")),
            (utc("2026-11-24T08:00:00Z"), utc("2026-11-24T17:00:00Z")),
        ];
        let now = utc("2026-11-23T13:07:00Z");
        let busy = vec![(utc("2026-11-24T08:00:00Z"), utc("2026-11-24T17:00:00Z"))];
        assert_eq!(
            free_slots(&windows, busy, now, 30),
            vec![(utc("2026-11-23T13:15:00Z"), utc("2026-11-23T17:00:00Z"))]
        );
        let late = utc("2026-11-23T16:50:00Z");
        assert!(free_slots(&windows[..1], vec![], late, 30).is_empty());
    }

    #[test]
    fn freebusy_parsing() {
        let (busy, errors) = parse_freebusy(&json!({"calendars": {
            "a@x.com": {"busy": [{"start": "2026-11-24T09:00:00Z", "end": "2026-11-24T10:00:00+01:00"}]},
            "b@x.com": {"errors": [{"domain": "global", "reason": "notFound"}]}
        }}));
        assert_eq!(
            busy,
            vec![(utc("2026-11-24T09:00:00Z"), utc("2026-11-24T09:00:00Z"))]
        );
        assert_eq!(errors, vec!["b@x.com: notFound"]);
    }

    #[test]
    fn calendar_selection() {
        let list = json!({"items": [
            {"id": "me@x.com", "summary": "me@x.com", "primary": true, "accessRole": "owner"},
            {"id": "work@group", "summary": "Work", "accessRole": "writer"},
            {"id": "en.ng#holiday@group.v.calendar.google.com", "summary": "Holidays", "accessRole": "reader"},
            {"id": "team@group", "summary": "Team", "summaryOverride": "My Team", "accessRole": "freeBusyReader"}
        ]});
        let ids = |v: Vec<(String, String)>| v.into_iter().map(|(i, _)| i).collect::<Vec<_>>();
        assert_eq!(
            ids(select_calendars(&list, &[], false)),
            vec!["me@x.com", "work@group"]
        );
        assert_eq!(select_calendars(&list, &[], true).len(), 4);
        assert_eq!(
            ids(select_calendars(&list, &["work".to_string()], false)),
            vec!["work@group"]
        );
        assert_eq!(
            select_calendars(&list, &["my team".to_string()], true)[0].1,
            "My Team"
        );
    }

    #[test]
    fn slot_rows_human_and_machine() {
        let slots = vec![(utc("2026-11-24T08:30:00Z"), utc("2026-11-24T10:00:00Z"))];
        let h = slot_rows(&slots, &wat(), true);
        assert_eq!(
            h[0],
            vec![
                json!("Tue 24 Nov"),
                json!("09:30"),
                json!("11:00"),
                json!("1h 30m")
            ]
        );
        let m = slot_rows(&slots, &wat(), false);
        assert_eq!(m[0][0], "2026-11-24");
        assert_eq!(m[0][1], "2026-11-24T09:30:00+01:00");
        assert_eq!(m[0][3], 90);
        assert_eq!(duration_text(45), "45m");
        assert_eq!(duration_text(120), "2h");
    }
    #[test]
    fn event_busy_rules() {
        let tz = wat();
        let timed = json!({"start": {"dateTime": "2026-11-24T10:00:00+01:00"},
                           "end": {"dateTime": "2026-11-24T11:00:00+01:00"}});
        assert_eq!(
            event_busy_interval(&timed, &tz, false),
            Some((utc("2026-11-24T09:00:00Z"), utc("2026-11-24T10:00:00Z")))
        );
        let mut free = timed.clone();
        free["transparency"] = json!("transparent");
        assert_eq!(event_busy_interval(&free, &tz, false), None);
        let mut declined = timed.clone();
        declined["attendees"] = json!([{"email": "o@x", "responseStatus": "accepted"},
                                       {"self": true, "responseStatus": "declined"}]);
        assert_eq!(event_busy_interval(&declined, &tz, false), None);
        let mut cancelled = timed.clone();
        cancelled["status"] = json!("cancelled");
        assert_eq!(event_busy_interval(&cancelled, &tz, false), None);

        let all_day = json!({"start": {"date": "2026-11-09"}, "end": {"date": "2026-12-19"}});
        assert_eq!(event_busy_interval(&all_day, &tz, false), None);
        assert_eq!(
            event_busy_interval(&all_day, &tz, true),
            Some((utc("2026-11-08T23:00:00Z"), utc("2026-12-18T23:00:00Z")))
        );
        let mut ooo = all_day.clone();
        ooo["eventType"] = json!("outOfOffice");
        assert!(event_busy_interval(&ooo, &tz, false).is_some());
    }
}

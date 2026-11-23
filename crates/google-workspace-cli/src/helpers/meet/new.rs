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

//! `gws meet +new`: create a meeting space and, with `--title`, a Calendar
//! event holding its link.

use super::api::{meet_url, MeetApi, CALENDAR_BASE};
use super::records::{parse_time, str_field};
use crate::error::GwsError;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// The validated inputs for `+new`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct NewMeeting {
    pub title: Option<String>,
    pub start: DateTime<Utc>,
    pub minutes: i64,
}

pub(super) fn parse_new(
    title: Option<&str>,
    start: Option<&str>,
    minutes: Option<&str>,
    now: DateTime<Utc>,
) -> Result<NewMeeting, GwsError> {
    let title = match title.map(str::trim) {
        Some("") => {
            return Err(GwsError::Validation(
                "--title must not be empty".to_string(),
            ));
        }
        Some(t) => {
            crate::validate::reject_dangerous_chars(t, "--title")?;
            Some(t.to_string())
        }
        None => None,
    };
    let start = match start {
        Some(s) => parse_time(s).ok_or_else(|| {
            GwsError::Validation(format!(
                "--start must be an RFC 3339 time such as 2026-06-17T09:00:00-07:00, got: {s}"
            ))
        })?,
        None => now,
    };
    let minutes = match minutes {
        Some(m) => m
            .parse::<i64>()
            .ok()
            .filter(|m| (1..=1440).contains(m))
            .ok_or_else(|| {
                GwsError::Validation(format!("--minutes must be 1 to 1440, got: {m}"))
            })?,
        None => 30,
    };
    Ok(NewMeeting {
        title,
        start,
        minutes,
    })
}

/// Calendar event body that carries an existing Meet space as its
/// conference, so Calendar shows the usual "Join with Google Meet" button.
pub(super) fn event_body(m: &NewMeeting, title: &str, uri: &str, code: &str) -> Value {
    let end = m.start + chrono::Duration::minutes(m.minutes);
    json!({
        "summary": title,
        "start": {"dateTime": m.start.to_rfc3339()},
        "end": {"dateTime": end.to_rfc3339()},
        "description": format!("Join with Google Meet: {uri}"),
        "conferenceData": {
            "conferenceId": code,
            "conferenceSolution": {"key": {"type": "hangoutsMeet"}},
            "entryPoints": [{
                "entryPointType": "video",
                "uri": uri,
                "label": uri.trim_start_matches("https://"),
            }],
        },
    })
}

/// What `--dry-run` prints: the requests that would be sent.
pub(super) fn dry_run_plan(m: &NewMeeting) -> Value {
    let mut requests = vec![json!({"method": "POST", "url": meet_url("spaces"), "body": {}})];
    if let Some(title) = &m.title {
        requests.push(json!({
            "method": "POST",
            "url": format!("{CALENDAR_BASE}/calendars/primary/events?conferenceDataVersion=1"),
            "body": event_body(m, title, "<meetingUri>", "<meetingCode>"),
        }));
    }
    json!({"dryRun": true, "requests": requests})
}

pub(super) async fn create(api: &dyn MeetApi, m: &NewMeeting) -> Result<Value, GwsError> {
    let space = api
        .post_json(&meet_url("spaces"), &vec![], &json!({}))
        .await?;
    let uri = str_field(&space, "meetingUri")
        .unwrap_or_default()
        .to_string();
    let code = str_field(&space, "meetingCode")
        .unwrap_or_default()
        .to_string();
    let mut out = json!({
        "space": str_field(&space, "name"),
        "meetingUri": uri,
        "meetingCode": code,
    });

    if let Some(title) = &m.title {
        let event = api
            .post_json(
                &format!("{CALENDAR_BASE}/calendars/primary/events"),
                &vec![("conferenceDataVersion", "1".to_string())],
                &event_body(m, title, &uri, &code),
            )
            .await
            .map_err(|e| {
                GwsError::Other(anyhow::anyhow!(
                    "Created meeting {uri}, but the Calendar event failed: {e}"
                ))
            })?;
        out["event"] = json!({
            "id": str_field(&event, "id"),
            "title": str_field(&event, "summary"),
            "htmlLink": str_field(&event, "htmlLink"),
            "start": event.get("start").and_then(|s| str_field(s, "dateTime")),
            "end": event.get("end").and_then(|s| str_field(s, "dateTime")),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::api::fake::FakeApi;
    use super::*;

    fn now() -> DateTime<Utc> {
        parse_time("2026-01-01T10:00:00Z").unwrap()
    }

    #[test]
    fn parses_defaults_and_values() {
        let m = parse_new(None, None, None, now()).unwrap();
        assert_eq!(
            m,
            NewMeeting {
                title: None,
                start: now(),
                minutes: 30
            }
        );
        let m = parse_new(
            Some(" Sync "),
            Some("2026-02-01T09:00:00+01:00"),
            Some("45"),
            now(),
        )
        .unwrap();
        assert_eq!(m.title.as_deref(), Some("Sync"));
        assert_eq!(m.start, parse_time("2026-02-01T08:00:00Z").unwrap());
        assert_eq!(m.minutes, 45);
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(parse_new(Some("  "), None, None, now()).is_err());
        assert!(parse_new(Some("a\u{202e}b"), None, None, now()).is_err());
        assert!(parse_new(None, Some("tomorrow"), None, now()).is_err());
        assert!(parse_new(None, None, Some("0"), now()).is_err());
        assert!(parse_new(None, None, Some("1441"), now()).is_err());
        assert!(parse_new(None, None, Some("ten"), now()).is_err());
    }

    #[test]
    fn event_body_carries_the_space() {
        let m = parse_new(Some("Sync"), None, Some("15"), now()).unwrap();
        let b = event_body(
            &m,
            "Sync",
            "https://meet.google.com/abc-defg-hij",
            "abc-defg-hij",
        );
        assert_eq!(b["end"]["dateTime"], "2026-01-01T10:15:00+00:00");
        assert_eq!(b["conferenceData"]["conferenceId"], "abc-defg-hij");
        assert_eq!(
            b["conferenceData"]["entryPoints"][0]["label"],
            "meet.google.com/abc-defg-hij"
        );
        assert!(b["description"].as_str().unwrap().contains("abc-defg-hij"));
    }

    #[test]
    fn dry_run_lists_requests() {
        let plain = dry_run_plan(&parse_new(None, None, None, now()).unwrap());
        assert_eq!(plain["requests"].as_array().unwrap().len(), 1);
        let titled = dry_run_plan(&parse_new(Some("Sync"), None, None, now()).unwrap());
        assert_eq!(titled["requests"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn creates_space_and_event() {
        let api = FakeApi::default()
            .with(
                &meet_url("spaces"),
                json!({"name": "spaces/s", "meetingUri": "https://meet.google.com/abc-defg-hij", "meetingCode": "abc-defg-hij"}),
            )
            .with(
                &format!("{CALENDAR_BASE}/calendars/primary/events"),
                json!({"id": "e1", "summary": "Sync", "htmlLink": "https://calendar/e1",
                       "start": {"dateTime": "2026-01-01T10:00:00Z"}, "end": {"dateTime": "2026-01-01T10:30:00Z"}}),
            );
        let m = parse_new(Some("Sync"), None, None, now()).unwrap();
        let out = create(&api, &m).await.unwrap();
        assert_eq!(out["meetingUri"], "https://meet.google.com/abc-defg-hij");
        assert_eq!(out["event"]["id"], "e1");
        let posts = api.posts.lock().unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[1].1["conferenceData"]["conferenceId"], "abc-defg-hij");
    }

    #[tokio::test]
    async fn space_only_without_title() {
        let api = FakeApi::default().with(
            &meet_url("spaces"),
            json!({"name": "spaces/s", "meetingUri": "u", "meetingCode": "c"}),
        );
        let out = create(&api, &parse_new(None, None, None, now()).unwrap())
            .await
            .unwrap();
        assert!(out.get("event").is_none());
        assert_eq!(api.posts.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn event_failure_reports_the_created_link() {
        let api = FakeApi::default().with(&meet_url("spaces"), json!({"name": "spaces/s", "meetingUri": "https://meet.google.com/x", "meetingCode": "c"}));
        let err = create(&api, &parse_new(Some("Sync"), None, None, now()).unwrap())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("https://meet.google.com/x"));
    }
}

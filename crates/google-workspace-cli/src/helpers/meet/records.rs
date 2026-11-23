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

//! Shared building blocks: resolving which conference record the user means,
//! spaces, participants, durations and Calendar event matching.

use super::api::{list_all, meet_url, MeetApi, Query, CALENDAR_BASE};
use crate::error::GwsError;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// What the user typed to identify a meeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RecordRef {
    /// The most recent conference record.
    Latest,
    /// A conference record resource name (`conferenceRecords/ID`).
    Name(String),
    /// A meeting code (`abc-defg-hij`), resolved to its newest record.
    MeetingCode(String),
}

/// True if `s` looks like a Meet meeting code: `xxx-xxxx-xxx`, lowercase letters.
pub(super) fn is_meeting_code(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 3
        && [3usize, 4, 3]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_lowercase()))
}

/// Parse a record argument: `latest`, a meeting code, a meet.google.com link,
/// `conferenceRecords/ID`, or a bare record ID.
pub(super) fn parse_record_ref(raw: &str) -> Result<RecordRef, GwsError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(GwsError::Validation(
            "A conference record, meeting code or \"latest\" is required".to_string(),
        ));
    }
    if s.eq_ignore_ascii_case("latest") {
        return Ok(RecordRef::Latest);
    }
    // Accept a pasted Meet link: https://meet.google.com/abc-defg-hij?authuser=0
    let link_tail = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    if let Some(rest) = link_tail.strip_prefix("meet.google.com/") {
        let code = rest
            .split(['?', '#', '/'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if is_meeting_code(&code) {
            return Ok(RecordRef::MeetingCode(code));
        }
        return Err(GwsError::Validation(format!(
            "Could not find a meeting code in link: {s}"
        )));
    }
    let lower = s.to_ascii_lowercase();
    if is_meeting_code(&lower) {
        return Ok(RecordRef::MeetingCode(lower));
    }
    let name = if s.starts_with("conferenceRecords/") {
        s.to_string()
    } else {
        format!("conferenceRecords/{s}")
    };
    crate::validate::validate_resource_name(&name)?;
    let segments: Vec<&str> = name.split('/').collect();
    if segments.len() != 2 || segments[1].is_empty() {
        return Err(GwsError::Validation(format!(
            "Expected conferenceRecords/ID, a meeting code (abc-defg-hij) or \"latest\", got: {s}"
        )));
    }
    Ok(RecordRef::Name(name))
}

pub(super) fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub(super) fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Seconds between two RFC 3339 timestamps. A missing end means "still
/// running" and is measured to `now`.
pub(super) fn span_seconds(start: Option<&str>, end: Option<&str>, now: DateTime<Utc>) -> i64 {
    let Some(start) = start.and_then(parse_time) else {
        return 0;
    };
    let end = end.and_then(parse_time).unwrap_or(now);
    (end - start).num_seconds().max(0)
}

/// Human duration: `1h 05m`, `12m 30s`, `45s`.
pub(super) fn format_duration(secs: i64) -> String {
    let secs = secs.max(0);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Pick the record with the latest `startTime`.
pub(super) fn pick_newest(records: &[Value]) -> Option<Value> {
    records
        .iter()
        .max_by_key(|r| str_field(r, "startTime").and_then(parse_time))
        .cloned()
}

/// Resolve a [`RecordRef`] to the conference record JSON.
pub(super) async fn resolve_record(api: &dyn MeetApi, r: &RecordRef) -> Result<Value, GwsError> {
    let list_url = meet_url("conferenceRecords");
    match r {
        RecordRef::Latest => {
            let page = api
                .get_json(&list_url, &vec![("pageSize", "1".to_string())])
                .await?;
            page.get("conferenceRecords")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .cloned()
                .ok_or_else(|| not_found("No conference records found for this account."))
        }
        RecordRef::MeetingCode(code) => {
            let query: Query = vec![
                ("filter", format!("space.meeting_code = \"{code}\"")),
                ("pageSize", "25".to_string()),
            ];
            let records = list_all(api, &list_url, "conferenceRecords", &query, Some(25)).await?;
            if records.len() > 1 {
                crate::output::info(&format!(
                    "{} conference records use meeting code {code}; using the newest. \
                     Run `gws meet +recent` to pick another.",
                    records.len()
                ));
            }
            pick_newest(&records).ok_or_else(|| {
                not_found(&format!(
                    "No conference record found for meeting code {code}."
                ))
            })
        }
        RecordRef::Name(name) => api.get_json(&meet_url(name), &vec![]).await,
    }
}

/// A "the data you asked for does not exist" error. Helpers use it instead of
/// printing an empty result, so scripts can tell "nothing there" from success.
pub(super) fn not_found(message: &str) -> GwsError {
    GwsError::Api {
        code: 404,
        message: message.to_string(),
        reason: "notFound".to_string(),
        enable_url: None,
    }
}

/// Meeting code and link for a space, if the caller may read it.
pub(super) async fn space_info(api: &dyn MeetApi, space: &str) -> (Option<String>, Option<String>) {
    if crate::validate::validate_resource_name(space).is_err() {
        return (None, None);
    }
    match api.get_json(&meet_url(space), &vec![]).await {
        Ok(v) => (
            str_field(&v, "meetingCode").map(str::to_string),
            str_field(&v, "meetingUri").map(str::to_string),
        ),
        Err(_) => (None, None),
    }
}

pub(super) async fn list_participants(
    api: &dyn MeetApi,
    record: &str,
) -> Result<Vec<Value>, GwsError> {
    list_all(
        api,
        &meet_url(&format!("{record}/participants")),
        "participants",
        &vec![("pageSize", "250".to_string())],
        None,
    )
    .await
}

/// `signedin`, `anonymous` or `phone`.
pub(super) fn participant_kind(p: &Value) -> &'static str {
    if p.get("signedinUser").is_some() {
        "signedin"
    } else if p.get("anonymousUser").is_some() {
        "anonymous"
    } else if p.get("phoneUser").is_some() {
        "phone"
    } else {
        "unknown"
    }
}

pub(super) fn participant_name(p: &Value) -> String {
    ["signedinUser", "anonymousUser", "phoneUser"]
        .iter()
        .filter_map(|k| p.get(*k))
        .find_map(|u| str_field(u, "displayName"))
        .filter(|n| !n.is_empty())
        .unwrap_or("(unknown)")
        .to_string()
}

/// True if a Calendar event carries this meeting code in its conference
/// data, Meet link, location or description.
pub(super) fn event_matches_code(event: &Value, code: &str) -> bool {
    let conf = event.get("conferenceData");
    if conf
        .and_then(|c| str_field(c, "conferenceId"))
        .is_some_and(|id| id.eq_ignore_ascii_case(code))
    {
        return true;
    }
    let needle = format!("meet.google.com/{code}");
    let entry_points = conf
        .and_then(|c| c.get("entryPoints"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| str_field(e, "uri"));
    let texts = ["hangoutLink", "location", "description"]
        .iter()
        .filter_map(|k| str_field(event, k));
    entry_points
        .chain(texts)
        .any(|t| t.to_ascii_lowercase().contains(&needle))
}

fn event_start(event: &Value) -> Option<DateTime<Utc>> {
    let start = event.get("start")?;
    str_field(start, "dateTime").and_then(parse_time)
}

/// The event holding `code` whose start is closest to the meeting's start.
pub(super) fn best_event<'a>(
    events: &'a [Value],
    code: &str,
    started: Option<DateTime<Utc>>,
) -> Option<&'a Value> {
    events
        .iter()
        .filter(|e| event_matches_code(e, code))
        .min_by_key(|e| match (event_start(e), started) {
            (Some(a), Some(b)) => (a - b).num_seconds().abs(),
            _ => i64::MAX,
        })
}

/// Events on the primary calendar that overlap `[time_min, time_max]`.
pub(super) async fn calendar_events(
    api: &dyn MeetApi,
    time_min: DateTime<Utc>,
    time_max: DateTime<Utc>,
) -> Result<Vec<Value>, GwsError> {
    let query: Query = vec![
        ("timeMin", time_min.to_rfc3339()),
        ("timeMax", time_max.to_rfc3339()),
        ("singleEvents", "true".to_string()),
        ("maxResults", "250".to_string()),
    ];
    list_all(
        api,
        &format!("{CALENDAR_BASE}/calendars/primary/events"),
        "items",
        &query,
        Some(2500),
    )
    .await
}

/// Find the Calendar event for one conference record (±1 hour window).
pub(super) async fn event_for_record(
    api: &dyn MeetApi,
    record: &Value,
    code: &str,
) -> Option<Value> {
    let start = str_field(record, "startTime").and_then(parse_time)?;
    let end = str_field(record, "endTime")
        .and_then(parse_time)
        .unwrap_or_else(Utc::now);
    let hour = chrono::Duration::hours(1);
    let events = calendar_events(api, start - hour, end + hour).await.ok()?;
    best_event(&events, code, Some(start)).cloned()
}

#[cfg(test)]
mod tests {
    use super::super::api::fake::FakeApi;
    use super::super::api::MEET_BASE;
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_latest_codes_links_and_names() {
        assert_eq!(parse_record_ref("latest").unwrap(), RecordRef::Latest);
        assert_eq!(parse_record_ref(" LATEST ").unwrap(), RecordRef::Latest);
        assert_eq!(
            parse_record_ref("abc-defg-hij").unwrap(),
            RecordRef::MeetingCode("abc-defg-hij".into())
        );
        assert_eq!(
            parse_record_ref("ABC-DEFG-HIJ").unwrap(),
            RecordRef::MeetingCode("abc-defg-hij".into())
        );
        assert_eq!(
            parse_record_ref("https://meet.google.com/abc-defg-hij?authuser=1").unwrap(),
            RecordRef::MeetingCode("abc-defg-hij".into())
        );
        assert_eq!(
            parse_record_ref("meet.google.com/abc-defg-hij").unwrap(),
            RecordRef::MeetingCode("abc-defg-hij".into())
        );
        assert_eq!(
            parse_record_ref("conferenceRecords/xyz").unwrap(),
            RecordRef::Name("conferenceRecords/xyz".into())
        );
        assert_eq!(
            parse_record_ref("xyz_-1").unwrap(),
            RecordRef::Name("conferenceRecords/xyz_-1".into())
        );
    }

    #[test]
    fn rejects_bad_record_refs() {
        for bad in [
            "",
            "   ",
            "conferenceRecords/../spaces",
            "conferenceRecords/a/participants",
            "a?b",
            "a#b",
            "%2e%2e",
            "a\nb",
            "conferenceRecords/",
            "https://meet.google.com/not-a-code",
        ] {
            assert!(parse_record_ref(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn meeting_code_shape() {
        assert!(is_meeting_code("abc-defg-hij"));
        assert!(!is_meeting_code("abc-def-hij"));
        assert!(!is_meeting_code("abc-defg-hi1"));
        assert!(!is_meeting_code("abcdefghij"));
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(750), "12m 30s");
        assert_eq!(format_duration(3900), "1h 05m");
        assert_eq!(format_duration(-5), "0s");
        let now = parse_time("2026-01-01T01:00:00Z").unwrap();
        assert_eq!(
            span_seconds(
                Some("2026-01-01T00:00:00.5Z"),
                Some("2026-01-01T00:10:00.5Z"),
                now
            ),
            600
        );
        assert_eq!(span_seconds(Some("2026-01-01T00:30:00Z"), None, now), 1800);
        assert_eq!(span_seconds(None, None, now), 0);
        assert_eq!(
            span_seconds(
                Some("2026-01-01T00:30:00Z"),
                Some("2026-01-01T00:00:00Z"),
                now
            ),
            0
        );
    }

    #[test]
    fn participant_names_and_kinds() {
        let s = json!({"signedinUser": {"displayName": "Ada", "user": "users/1"}});
        let a = json!({"anonymousUser": {"displayName": "Guest"}});
        let p = json!({"phoneUser": {"displayName": "+1 555"}});
        let u = json!({"name": "x"});
        assert_eq!(participant_kind(&s), "signedin");
        assert_eq!(participant_kind(&a), "anonymous");
        assert_eq!(participant_kind(&p), "phone");
        assert_eq!(participant_kind(&u), "unknown");
        assert_eq!(participant_name(&s), "Ada");
        assert_eq!(participant_name(&a), "Guest");
        assert_eq!(participant_name(&u), "(unknown)");
        assert_eq!(
            participant_name(&json!({"signedinUser": {"displayName": ""}})),
            "(unknown)"
        );
    }

    #[test]
    fn matches_events_by_code_in_any_field() {
        let code = "abc-defg-hij";
        let by_id = json!({"conferenceData": {"conferenceId": "abc-defg-hij"}});
        let by_link = json!({"hangoutLink": "https://meet.google.com/abc-defg-hij"});
        let by_entry = json!({"conferenceData": {"entryPoints": [{"uri": "https://meet.google.com/abc-defg-hij"}]}});
        let by_loc = json!({"location": "https://meet.google.com/ABC-DEFG-HIJ"});
        let other = json!({"hangoutLink": "https://meet.google.com/zzz-zzzz-zzz"});
        assert!(event_matches_code(&by_id, code));
        assert!(event_matches_code(&by_link, code));
        assert!(event_matches_code(&by_entry, code));
        assert!(event_matches_code(&by_loc, code));
        assert!(!event_matches_code(&other, code));
        assert!(!event_matches_code(&json!({}), code));
    }

    #[test]
    fn best_event_prefers_closest_start() {
        let code = "abc-defg-hij";
        let events = vec![
            json!({"summary": "far", "hangoutLink": "https://meet.google.com/abc-defg-hij", "start": {"dateTime": "2026-01-01T08:00:00Z"}}),
            json!({"summary": "near", "hangoutLink": "https://meet.google.com/abc-defg-hij", "start": {"dateTime": "2026-01-01T09:55:00Z"}}),
            json!({"summary": "other", "start": {"dateTime": "2026-01-01T10:00:00Z"}}),
        ];
        let started = parse_time("2026-01-01T10:00:00Z");
        assert_eq!(
            best_event(&events, code, started).unwrap()["summary"],
            "near"
        );
        assert!(best_event(&events, "zzz-zzzz-zzz", started).is_none());
    }

    #[test]
    fn pick_newest_by_start() {
        let recs = vec![
            json!({"name": "a", "startTime": "2026-01-01T00:00:00Z"}),
            json!({"name": "b", "startTime": "2026-02-01T00:00:00Z"}),
        ];
        assert_eq!(pick_newest(&recs).unwrap()["name"], "b");
        assert!(pick_newest(&[]).is_none());
    }

    #[tokio::test]
    async fn resolves_latest() {
        let url = format!("{MEET_BASE}/conferenceRecords");
        let api = FakeApi::default().with(
            &url,
            json!({"conferenceRecords": [{"name": "conferenceRecords/new"}]}),
        );
        let r = resolve_record(&api, &RecordRef::Latest).await.unwrap();
        assert_eq!(r["name"], "conferenceRecords/new");
    }

    #[tokio::test]
    async fn latest_with_no_records_is_an_error() {
        let url = format!("{MEET_BASE}/conferenceRecords");
        let api = FakeApi::default().with(&url, json!({}));
        let err = resolve_record(&api, &RecordRef::Latest).await.unwrap_err();
        assert!(err.to_string().contains("No conference records"));
    }

    #[tokio::test]
    async fn resolves_meeting_code_to_newest_with_filter() {
        let url = format!("{MEET_BASE}/conferenceRecords");
        let api = FakeApi::default().with(
            &url,
            json!({"conferenceRecords": [
                {"name": "conferenceRecords/old", "startTime": "2026-01-01T00:00:00Z"},
                {"name": "conferenceRecords/new", "startTime": "2026-02-01T00:00:00Z"}
            ]}),
        );
        let r = resolve_record(&api, &RecordRef::MeetingCode("abc-defg-hij".into()))
            .await
            .unwrap();
        assert_eq!(r["name"], "conferenceRecords/new");
        let q = &api.queries_for(&url)[0];
        assert!(q.contains(&(
            "filter",
            "space.meeting_code = \"abc-defg-hij\"".to_string()
        )));
    }

    #[tokio::test]
    async fn unknown_meeting_code_is_an_error() {
        let url = format!("{MEET_BASE}/conferenceRecords");
        let api = FakeApi::default().with(&url, json!({"conferenceRecords": []}));
        assert!(
            resolve_record(&api, &RecordRef::MeetingCode("abc-defg-hij".into()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn resolves_name_directly() {
        let api = FakeApi::default().with(
            &format!("{MEET_BASE}/conferenceRecords/r1"),
            json!({"name": "conferenceRecords/r1"}),
        );
        let r = resolve_record(&api, &RecordRef::Name("conferenceRecords/r1".into()))
            .await
            .unwrap();
        assert_eq!(r["name"], "conferenceRecords/r1");
    }

    #[tokio::test]
    async fn space_info_reads_code_and_tolerates_errors() {
        let api = FakeApi::default().with(
            &format!("{MEET_BASE}/spaces/s1"),
            json!({"meetingCode": "abc-defg-hij", "meetingUri": "https://meet.google.com/abc-defg-hij"}),
        );
        let (code, uri) = space_info(&api, "spaces/s1").await;
        assert_eq!(code.as_deref(), Some("abc-defg-hij"));
        assert!(uri.unwrap().ends_with("abc-defg-hij"));
        assert_eq!(space_info(&api, "spaces/missing").await, (None, None));
        assert_eq!(space_info(&api, "spaces/../x").await, (None, None));
    }

    #[tokio::test]
    async fn finds_event_for_record() {
        let api = FakeApi::default().with(
            &format!("{CALENDAR_BASE}/calendars/primary/events"),
            json!({"items": [
                {"summary": "Standup", "hangoutLink": "https://meet.google.com/abc-defg-hij", "start": {"dateTime": "2026-01-01T10:00:00Z"}}
            ]}),
        );
        let rec = json!({"startTime": "2026-01-01T10:02:00Z", "endTime": "2026-01-01T10:30:00Z"});
        let ev = event_for_record(&api, &rec, "abc-defg-hij").await.unwrap();
        assert_eq!(ev["summary"], "Standup");
        assert!(event_for_record(&api, &json!({}), "abc-defg-hij")
            .await
            .is_none());
    }
}

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

//! `gws meet +recent`: recent conference records joined with their meeting
//! code, Calendar event title and participant count.

use super::api::{list_all, meet_url, MeetApi, Query};
use super::records::{
    best_event, calendar_events, format_duration, not_found, parse_time, space_info, span_seconds,
    str_field,
};
use crate::error::GwsError;
use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use serde_json::{json, Value};

/// Participant count for a record, from `totalSize` when the API reports it.
async fn participant_count(api: &dyn MeetApi, record: &str) -> Option<u64> {
    let url = meet_url(&format!("{record}/participants"));
    let page = api
        .get_json(&url, &vec![("pageSize", "250".to_string())])
        .await
        .ok()?;
    if let Some(n) = page
        .get("totalSize")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
    {
        return Some(n);
    }
    let first = page
        .get("participants")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if page.get("nextPageToken").and_then(Value::as_str).is_none() {
        return Some(first as u64);
    }
    super::records::list_participants(api, record)
        .await
        .ok()
        .map(|v| v.len() as u64)
}

pub(super) async fn recent(
    api: &dyn MeetApi,
    days: i64,
    limit: usize,
    now: DateTime<Utc>,
) -> Result<Value, GwsError> {
    let since = now - chrono::Duration::days(days);
    let query: Query = vec![
        (
            "filter",
            format!("start_time>=\"{}\"", since.format("%Y-%m-%dT%H:%M:%SZ")),
        ),
        ("pageSize", limit.min(100).to_string()),
    ];
    let mut records = list_all(
        api,
        &meet_url("conferenceRecords"),
        "conferenceRecords",
        &query,
        Some(limit),
    )
    .await?;
    if records.is_empty() {
        return Err(not_found(&format!(
            "No meetings found in the last {days} day(s)."
        )));
    }
    records.sort_by_key(|r| std::cmp::Reverse(str_field(r, "startTime").and_then(parse_time)));

    // One Calendar query for the whole window; a failure only blanks titles.
    let hour = chrono::Duration::hours(1);
    let events = match calendar_events(api, since - hour, now + hour).await {
        Ok(e) => e,
        Err(e) => {
            crate::output::warn(&format!("Calendar lookup failed, titles omitted: {e}"));
            Vec::new()
        }
    };

    let rows: Vec<Value> = stream::iter(records)
        .map(|rec| {
            let events = &events;
            async move {
                let name = str_field(&rec, "name").unwrap_or_default().to_string();
                let space = str_field(&rec, "space").unwrap_or_default();
                let (code, _) = space_info(api, space).await;
                let participants = participant_count(api, &name).await;
                let start = str_field(&rec, "startTime");
                let end = str_field(&rec, "endTime");
                let title = code.as_deref().and_then(|c| {
                    best_event(events, c, start.and_then(parse_time))
                        .and_then(|e| str_field(e, "summary"))
                        .map(str::to_string)
                });
                let secs = span_seconds(start, end, now);
                json!({
                    "start": start,
                    "end": end,
                    "live": end.is_none(),
                    "duration": format_duration(secs),
                    "durationSeconds": secs,
                    "meetingCode": code,
                    "title": title,
                    "participants": participants,
                    "record": name,
                })
            }
        })
        .buffered(5)
        .collect()
        .await;

    Ok(json!({
        "count": rows.len(),
        "days": days,
        "meetings": rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::api::fake::FakeApi;
    use super::super::api::{CALENDAR_BASE, MEET_BASE};
    use super::*;

    fn now() -> DateTime<Utc> {
        parse_time("2026-01-10T12:00:00Z").unwrap()
    }

    fn api() -> FakeApi {
        FakeApi::default()
            .with(
                &format!("{MEET_BASE}/conferenceRecords"),
                json!({"conferenceRecords": [
                    {"name": "conferenceRecords/a", "space": "spaces/s1",
                     "startTime": "2026-01-08T10:00:00Z", "endTime": "2026-01-08T10:45:00Z"},
                    {"name": "conferenceRecords/b", "space": "spaces/s2",
                     "startTime": "2026-01-09T10:00:00Z"}
                ]}),
            )
            .with(
                &format!("{MEET_BASE}/spaces/s1"),
                json!({"meetingCode": "abc-defg-hij"}),
            )
            .with(
                &format!("{MEET_BASE}/spaces/s2"),
                json!({"meetingCode": "zzz-yyyy-xxx"}),
            )
            .with(
                &format!("{MEET_BASE}/conferenceRecords/a/participants"),
                json!({"participants": [{}, {}], "totalSize": 2}),
            )
            .with(
                &format!("{MEET_BASE}/conferenceRecords/b/participants"),
                json!({"participants": [{}]}),
            )
            .with(
                &format!("{CALENDAR_BASE}/calendars/primary/events"),
                json!({"items": [{"summary": "Planning",
                    "hangoutLink": "https://meet.google.com/abc-defg-hij",
                    "start": {"dateTime": "2026-01-08T10:00:00Z"}}]}),
            )
    }

    #[tokio::test]
    async fn lists_newest_first_with_titles_and_counts() {
        let api = api();
        let out = recent(&api, 7, 10, now()).await.unwrap();
        let m = out["meetings"].as_array().unwrap();
        assert_eq!(out["count"], 2);
        assert_eq!(m[0]["record"], "conferenceRecords/b");
        assert_eq!(m[0]["live"], true);
        assert_eq!(m[0]["participants"], 1);
        assert_eq!(m[0]["title"], Value::Null);
        assert_eq!(m[1]["title"], "Planning");
        assert_eq!(m[1]["duration"], "45m 00s");
        assert_eq!(m[1]["participants"], 2);
        assert_eq!(m[1]["meetingCode"], "abc-defg-hij");

        let q = &api.queries_for(&format!("{MEET_BASE}/conferenceRecords"))[0];
        assert!(q.contains(&("filter", "start_time>=\"2026-01-03T12:00:00Z\"".to_string())));
    }

    #[tokio::test]
    async fn empty_window_is_an_error() {
        let api = FakeApi::default().with(&format!("{MEET_BASE}/conferenceRecords"), json!({}));
        let err = recent(&api, 3, 10, now()).await.unwrap_err();
        assert!(err.to_string().contains("last 3 day"));
    }

    #[tokio::test]
    async fn calendar_failure_only_blanks_titles() {
        let mut api = api();
        api.json
            .remove(&format!("{CALENDAR_BASE}/calendars/primary/events"));
        let out = recent(&api, 7, 10, now()).await.unwrap();
        assert_eq!(out["meetings"][1]["title"], Value::Null);
    }

    #[tokio::test]
    async fn participant_count_pages_when_total_missing() {
        let url = format!("{MEET_BASE}/conferenceRecords/c/participants");
        let api = FakeApi::default()
            .with(&url, json!({"participants": [{}], "nextPageToken": "p2"}))
            .with(&format!("{url}#p2"), json!({"participants": [{}, {}]}));
        assert_eq!(
            participant_count(&api, "conferenceRecords/c").await,
            Some(3)
        );
        assert_eq!(
            participant_count(&api, "conferenceRecords/none").await,
            None
        );
    }
}

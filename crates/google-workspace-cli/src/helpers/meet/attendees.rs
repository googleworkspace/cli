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

//! `gws meet +attendees`: who joined, when, and for how long in total.

use super::api::{list_all, meet_url, MeetApi};
use super::records::{
    format_duration, list_participants, not_found, parse_time, participant_kind, participant_name,
    resolve_record, space_info, span_seconds, str_field, RecordRef,
};
use crate::error::GwsError;
use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use serde_json::{json, Value};

/// Time a participant was in the call: the union of their sessions, so
/// joining from two devices at once is not counted twice. Open sessions run
/// to `now`.
pub(super) fn total_session_seconds(sessions: &[Value], now: DateTime<Utc>) -> i64 {
    let mut spans: Vec<(DateTime<Utc>, DateTime<Utc>)> = sessions
        .iter()
        .filter_map(|s| {
            let start = str_field(s, "startTime").and_then(parse_time)?;
            let end = str_field(s, "endTime").and_then(parse_time).unwrap_or(now);
            (end > start).then_some((start, end))
        })
        .collect();
    spans.sort();
    let mut total = 0;
    let mut current: Option<(DateTime<Utc>, DateTime<Utc>)> = None;
    for (start, end) in spans {
        current = match current {
            Some((cs, ce)) if start <= ce => Some((cs, ce.max(end))),
            Some((cs, ce)) => {
                total += (ce - cs).num_seconds();
                Some((start, end))
            }
            None => Some((start, end)),
        };
    }
    if let Some((cs, ce)) = current {
        total += (ce - cs).num_seconds();
    }
    total
}

pub(super) async fn attendees(
    api: &dyn MeetApi,
    r: &RecordRef,
    now: DateTime<Utc>,
) -> Result<Value, GwsError> {
    let record = resolve_record(api, r).await?;
    let name = str_field(&record, "name").unwrap_or_default().to_string();
    let (code, _) = space_info(api, str_field(&record, "space").unwrap_or_default()).await;
    let participants = list_participants(api, &name).await?;
    if participants.is_empty() {
        return Err(not_found(&format!("No participants recorded for {name}.")));
    }

    let mut rows: Vec<Value> = stream::iter(participants)
        .map(|p| async move {
            let pname = str_field(&p, "name").unwrap_or_default().to_string();
            let sessions = list_all(
                api,
                &meet_url(&format!("{pname}/participantSessions")),
                "participantSessions",
                &vec![("pageSize", "250".to_string())],
                None,
            )
            .await;
            // Fall back to the participant's own first/last span when the
            // session list is unavailable.
            let (count, secs) = match &sessions {
                Ok(s) if !s.is_empty() => (Some(s.len()), total_session_seconds(s, now)),
                _ => (
                    None,
                    span_seconds(
                        str_field(&p, "earliestStartTime"),
                        str_field(&p, "latestEndTime"),
                        now,
                    ),
                ),
            };
            json!({
                "name": participant_name(&p),
                "kind": participant_kind(&p),
                "firstJoin": str_field(&p, "earliestStartTime"),
                "lastLeave": str_field(&p, "latestEndTime"),
                "stillIn": str_field(&p, "latestEndTime").is_none(),
                "sessions": count,
                "totalTime": format_duration(secs),
                "totalSeconds": secs,
                "user": p.get("signedinUser").and_then(|u| str_field(u, "user")),
                "participant": pname,
            })
        })
        .buffered(5)
        .collect()
        .await;
    rows.sort_by(|a, b| {
        str_field(a, "firstJoin")
            .unwrap_or("")
            .cmp(str_field(b, "firstJoin").unwrap_or(""))
    });

    let signed_in = rows.iter().filter(|r| r["kind"] == "signedin").count();
    Ok(json!({
        "record": name,
        "meetingCode": code,
        "start": str_field(&record, "startTime"),
        "end": str_field(&record, "endTime"),
        "count": rows.len(),
        "signedIn": signed_in,
        "attendees": rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::api::fake::FakeApi;
    use super::super::api::MEET_BASE;
    use super::*;

    fn now() -> DateTime<Utc> {
        parse_time("2026-01-01T12:00:00Z").unwrap()
    }

    fn base() -> FakeApi {
        FakeApi::default()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r"),
                json!({"name": "conferenceRecords/r", "space": "spaces/s",
                       "startTime": "2026-01-01T10:00:00Z"}),
            )
            .with(
                &format!("{MEET_BASE}/spaces/s"),
                json!({"meetingCode": "abc-defg-hij"}),
            )
    }

    #[test]
    fn sums_sessions() {
        let s = vec![
            json!({"startTime": "2026-01-01T10:00:00Z", "endTime": "2026-01-01T10:10:00Z"}),
            json!({"startTime": "2026-01-01T11:50:00Z"}),
        ];
        assert_eq!(total_session_seconds(&s, now()), 1200);
        assert_eq!(total_session_seconds(&[], now()), 0);
    }

    #[test]
    fn overlapping_sessions_count_once() {
        // Two devices at once (10:00-10:30 and 10:10-10:20), then a rejoin.
        let s = vec![
            json!({"startTime": "2026-01-01T10:10:00Z", "endTime": "2026-01-01T10:20:00Z"}),
            json!({"startTime": "2026-01-01T10:00:00Z", "endTime": "2026-01-01T10:30:00Z"}),
            json!({"startTime": "2026-01-01T10:30:00Z", "endTime": "2026-01-01T10:40:00Z"}),
            json!({"startTime": "2026-01-01T11:00:00Z", "endTime": "2026-01-01T11:05:00Z"}),
            json!({"endTime": "2026-01-01T11:05:00Z"}),
        ];
        assert_eq!(total_session_seconds(&s, now()), 2400 + 300);
    }

    #[tokio::test]
    async fn lists_attendees_with_session_totals() {
        let api = base()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/participants"),
                json!({"participants": [
                    {"name": "conferenceRecords/r/participants/2",
                     "anonymousUser": {"displayName": "Guest"},
                     "earliestStartTime": "2026-01-01T10:05:00Z",
                     "latestEndTime": "2026-01-01T10:20:00Z"},
                    {"name": "conferenceRecords/r/participants/1",
                     "signedinUser": {"displayName": "Ada", "user": "users/1"},
                     "earliestStartTime": "2026-01-01T10:00:00Z",
                     "latestEndTime": "2026-01-01T11:00:00Z"}
                ]}),
            )
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/participants/1/participantSessions"),
                json!({"participantSessions": [
                    {"startTime": "2026-01-01T10:00:00Z", "endTime": "2026-01-01T10:30:00Z"},
                    {"startTime": "2026-01-01T10:40:00Z", "endTime": "2026-01-01T11:00:00Z"}
                ]}),
            );
        let out = attendees(&api, &RecordRef::Name("conferenceRecords/r".into()), now())
            .await
            .unwrap();
        assert_eq!(out["count"], 2);
        assert_eq!(out["signedIn"], 1);
        assert_eq!(out["meetingCode"], "abc-defg-hij");
        let a = &out["attendees"];
        assert_eq!(a[0]["name"], "Ada");
        assert_eq!(a[0]["kind"], "signedin");
        assert_eq!(a[0]["sessions"], 2);
        assert_eq!(a[0]["totalSeconds"], 3000);
        assert_eq!(a[0]["user"], "users/1");
        // Session list missing: falls back to first-join/last-leave span.
        assert_eq!(a[1]["name"], "Guest");
        assert_eq!(a[1]["sessions"], Value::Null);
        assert_eq!(a[1]["totalSeconds"], 900);
        assert_eq!(a[1]["stillIn"], false);
    }

    #[tokio::test]
    async fn no_participants_is_an_error() {
        let api = base().with(
            &format!("{MEET_BASE}/conferenceRecords/r/participants"),
            json!({}),
        );
        let err = attendees(&api, &RecordRef::Name("conferenceRecords/r".into()), now())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No participants"));
    }
}

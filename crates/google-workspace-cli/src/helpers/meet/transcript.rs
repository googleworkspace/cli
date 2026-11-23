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

//! `gws meet +transcript`: speaker-labelled transcript text.
//!
//! Entries come from `conferenceRecords.transcripts.entries`, with speakers
//! resolved through the participant list. When the API holds no entries
//! (they expire before the Doc does), the transcript's Google Doc is
//! exported through Drive instead.

use super::api::{export_doc_text, list_all, meet_url, MeetApi};
use super::records::{
    list_participants, not_found, parse_time, participant_name, resolve_record, str_field,
    RecordRef,
};
use crate::error::GwsError;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;

/// `HH:MM:SS` offset of `at` from the meeting start.
pub(super) fn offset_label(start: Option<DateTime<Utc>>, at: Option<&str>) -> String {
    let secs = match (start, at.and_then(parse_time)) {
        (Some(s), Some(t)) => (t - s).num_seconds().max(0),
        _ => return "--:--:--".to_string(),
    };
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

pub(super) async fn transcript(api: &dyn MeetApi, r: &RecordRef) -> Result<Value, GwsError> {
    let record = resolve_record(api, r).await?;
    let name = str_field(&record, "name").unwrap_or_default().to_string();
    let started = str_field(&record, "startTime").and_then(parse_time);

    let mut transcripts = list_all(
        api,
        &meet_url(&format!("{name}/transcripts")),
        "transcripts",
        &vec![],
        None,
    )
    .await?;
    if transcripts.is_empty() {
        return Err(not_found(&format!(
            "No transcript for {name}. Meet only keeps one when someone turned on \
             transcription during the call."
        )));
    }
    transcripts.sort_by(|a, b| {
        str_field(a, "startTime")
            .unwrap_or("")
            .cmp(str_field(b, "startTime").unwrap_or(""))
    });

    // Participant resource name -> display name, fetched once.
    let speakers: HashMap<String, String> = list_participants(api, &name)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|p| Some((str_field(p, "name")?.to_string(), participant_name(p))))
        .collect();

    let mut parts = Vec::new();
    for t in &transcripts {
        let tname = str_field(t, "name").unwrap_or_default();
        let doc = t
            .get("docsDestination")
            .and_then(|d| str_field(d, "document"))
            .map(str::to_string);
        let entries = list_all(
            api,
            &meet_url(&format!("{tname}/entries")),
            "transcriptEntries",
            &vec![("pageSize", "100".to_string())],
            None,
        )
        .await
        .unwrap_or_default();

        let (source, lines, text) = if !entries.is_empty() {
            let lines: Vec<Value> = entries
                .iter()
                .map(|e| {
                    let speaker = str_field(e, "participant")
                        .and_then(|p| speakers.get(p).cloned())
                        .unwrap_or_else(|| "(unknown)".to_string());
                    json!({
                        "offset": offset_label(started, str_field(e, "startTime")),
                        "start": str_field(e, "startTime"),
                        "speaker": speaker,
                        "text": str_field(e, "text").unwrap_or(""),
                    })
                })
                .collect();
            let text = lines
                .iter()
                .map(|l| {
                    format!(
                        "[{}] {}: {}",
                        l["offset"].as_str().unwrap_or(""),
                        l["speaker"].as_str().unwrap_or(""),
                        l["text"].as_str().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            ("entries", lines, Some(text))
        } else if let Some(doc_id) = &doc {
            match export_doc_text(api, doc_id).await {
                Ok(text) => ("document", Vec::new(), Some(text)),
                Err(e) => {
                    crate::output::warn(&format!("Could not export transcript doc {doc_id}: {e}"));
                    ("none", Vec::new(), None)
                }
            }
        } else {
            ("none", Vec::new(), None)
        };

        parts.push(json!({
            "transcript": tname,
            "state": str_field(t, "state"),
            "document": doc,
            "source": source,
            "entries": lines,
            "text": text,
        }));
    }

    if parts.iter().all(|p| p["text"].is_null()) {
        let states: Vec<&str> = transcripts
            .iter()
            .filter_map(|t| str_field(t, "state"))
            .collect();
        return Err(not_found(&format!(
            "{name} has {} transcript(s) but no text is available yet (state: {}). \
             Try again once Meet has finished writing the transcript.",
            transcripts.len(),
            if states.is_empty() {
                "unknown".to_string()
            } else {
                states.join(", ")
            }
        )));
    }

    Ok(json!({
        "record": name,
        "start": str_field(&record, "startTime"),
        "transcripts": parts,
    }))
}

/// Plain text for `--format text`: each transcript's text, separated.
pub(super) fn render_text(out: &Value) -> String {
    out["transcripts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::super::api::fake::FakeApi;
    use super::super::api::{DRIVE_BASE, MEET_BASE};
    use super::*;

    fn rec() -> RecordRef {
        RecordRef::Name("conferenceRecords/r".into())
    }

    fn base() -> FakeApi {
        FakeApi::default()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r"),
                json!({"name": "conferenceRecords/r", "startTime": "2026-01-01T10:00:00Z"}),
            )
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/participants"),
                json!({"participants": [
                    {"name": "conferenceRecords/r/participants/1", "signedinUser": {"displayName": "Ada"}}
                ]}),
            )
    }

    #[test]
    fn offsets() {
        let s = parse_time("2026-01-01T10:00:00Z");
        assert_eq!(offset_label(s, Some("2026-01-01T11:02:03.5Z")), "01:02:03");
        assert_eq!(offset_label(s, Some("2026-01-01T09:00:00Z")), "00:00:00");
        assert_eq!(offset_label(None, Some("2026-01-01T10:00:00Z")), "--:--:--");
        assert_eq!(offset_label(s, None), "--:--:--");
    }

    #[tokio::test]
    async fn labels_entries_with_speaker_names() {
        let api = base()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/transcripts"),
                json!({"transcripts": [{"name": "conferenceRecords/r/transcripts/t", "state": "FILE_GENERATED"}]}),
            )
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/transcripts/t/entries"),
                json!({"transcriptEntries": [
                    {"participant": "conferenceRecords/r/participants/1", "text": "Hello", "startTime": "2026-01-01T10:00:05Z"},
                    {"participant": "conferenceRecords/r/participants/9", "text": "Hi", "startTime": "2026-01-01T10:01:00Z"}
                ]}),
            );
        let out = transcript(&api, &rec()).await.unwrap();
        assert_eq!(
            render_text(&out),
            "[00:00:05] Ada: Hello\n[00:01:00] (unknown): Hi"
        );
        assert_eq!(out["transcripts"][0]["source"], "entries");
    }

    #[tokio::test]
    async fn falls_back_to_doc_export() {
        let api = base()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/transcripts"),
                json!({"transcripts": [{"name": "conferenceRecords/r/transcripts/t",
                    "docsDestination": {"document": "doc1"}}]}),
            )
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/transcripts/t/entries"),
                json!({}),
            )
            .with_text(&format!("{DRIVE_BASE}/files/doc1/export"), "Ada: Hello");
        let out = transcript(&api, &rec()).await.unwrap();
        assert_eq!(render_text(&out), "Ada: Hello");
        assert_eq!(out["transcripts"][0]["source"], "document");
    }

    #[tokio::test]
    async fn no_transcripts_is_an_error() {
        let api = base().with(
            &format!("{MEET_BASE}/conferenceRecords/r/transcripts"),
            json!({}),
        );
        let err = transcript(&api, &rec()).await.unwrap_err();
        assert!(err.to_string().contains("No transcript"));
    }

    #[tokio::test]
    async fn transcript_without_text_is_an_error() {
        let api = base().with(
            &format!("{MEET_BASE}/conferenceRecords/r/transcripts"),
            json!({"transcripts": [
                {"name": "conferenceRecords/r/transcripts/t", "state": "STARTED"},
                {"name": "conferenceRecords/r/transcripts/u", "docsDestination": {"document": "gone"}}
            ]}),
        );
        let err = transcript(&api, &rec()).await.unwrap_err();
        assert!(err.to_string().contains("no text is available"));
        assert!(err.to_string().contains("STARTED"));
    }
}

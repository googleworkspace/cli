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

//! `gws meet +notes`: Gemini "Take notes for me" notes as text.
//!
//! Smart notes come from `conferenceRecords.smartNotes`. When the API lists
//! none, the notes Doc that Gemini attaches to the Calendar event is used.

use super::api::{export_doc_text, list_all, meet_url, MeetApi};
use super::records::{
    event_for_record, not_found, resolve_record, space_info, str_field, RecordRef,
};
use crate::error::GwsError;
use serde_json::{json, Value};

const GOOGLE_DOC_MIME: &str = "application/vnd.google-apps.document";

/// Calendar attachments that look like meeting notes: Google Docs whose
/// title mentions notes or Gemini.
pub(super) fn notes_attachments(event: &Value) -> Vec<(String, String)> {
    event
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| str_field(a, "mimeType") == Some(GOOGLE_DOC_MIME))
        .filter_map(|a| {
            let title = str_field(a, "title").unwrap_or("");
            let lower = title.to_ascii_lowercase();
            if !(lower.contains("notes") || lower.contains("gemini")) {
                return None;
            }
            Some((str_field(a, "fileId")?.to_string(), title.to_string()))
        })
        .collect()
}

pub(super) async fn notes(api: &dyn MeetApi, r: &RecordRef) -> Result<Value, GwsError> {
    let record = resolve_record(api, r).await?;
    let name = str_field(&record, "name").unwrap_or_default().to_string();

    let mut checked = Vec::new();
    let mut pending = Vec::new();
    let mut documents = Vec::new();

    // 1. Smart notes from the Meet API.
    match list_all(
        api,
        &meet_url(&format!("{name}/smartNotes")),
        "smartNotes",
        &vec![],
        None,
    )
    .await
    {
        Ok(smart) => {
            checked.push(format!("{} smart note(s) in the Meet API", smart.len()));
            for n in &smart {
                let doc = n
                    .get("docsDestination")
                    .and_then(|d| str_field(d, "document"));
                match doc {
                    Some(id) => match export_doc_text(api, id).await {
                        Ok(text) => documents.push(json!({
                            "source": "smartNotes",
                            "smartNote": str_field(n, "name"),
                            "document": id,
                            "url": n.get("docsDestination").and_then(|d| str_field(d, "exportUri")),
                            "text": text,
                        })),
                        Err(e) => crate::output::warn(&format!(
                            "Could not export smart notes doc {id}: {e}"
                        )),
                    },
                    None => pending.push(str_field(n, "state").unwrap_or("UNKNOWN").to_string()),
                }
            }
        }
        Err(e) => checked.push(format!("smart notes unavailable ({e})")),
    }

    // 2. The notes Doc attached to the Calendar event.
    if documents.is_empty() {
        let (code, _) = space_info(api, str_field(&record, "space").unwrap_or_default()).await;
        match code {
            Some(code) => match event_for_record(api, &record, &code).await {
                Some(event) => {
                    let found = notes_attachments(&event);
                    checked.push(format!(
                        "{} notes attachment(s) on the Calendar event",
                        found.len()
                    ));
                    for (id, title) in found {
                        match export_doc_text(api, &id).await {
                            Ok(text) => documents.push(json!({
                                "source": "calendarAttachment",
                                "title": title,
                                "document": id,
                                "url": format!("https://docs.google.com/document/d/{id}"),
                                "text": text,
                            })),
                            Err(e) => crate::output::warn(&format!(
                                "Could not export notes doc {id}: {e}"
                            )),
                        }
                    }
                }
                None => checked.push(format!("no Calendar event holds meeting {code}")),
            },
            None => checked.push("meeting code unavailable, Calendar not searched".to_string()),
        }
    }

    if documents.is_empty() {
        let mut msg = format!("No meeting notes found for {name}");
        if !pending.is_empty() {
            msg.push_str(&format!(
                ". Gemini is still writing them (state: {})",
                pending.join(", ")
            ));
        }
        msg.push_str(&format!(". Checked: {}.", checked.join("; ")));
        return Err(not_found(&msg));
    }

    Ok(json!({
        "record": name,
        "start": str_field(&record, "startTime"),
        "notes": documents,
    }))
}

pub(super) fn render_text(out: &Value) -> String {
    out["notes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::super::api::fake::FakeApi;
    use super::super::api::{CALENDAR_BASE, DRIVE_BASE, MEET_BASE};
    use super::*;

    fn rec() -> RecordRef {
        RecordRef::Name("conferenceRecords/r".into())
    }

    fn base() -> FakeApi {
        FakeApi::default()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r"),
                json!({"name": "conferenceRecords/r", "space": "spaces/s",
                       "startTime": "2026-01-01T10:00:00Z", "endTime": "2026-01-01T11:00:00Z"}),
            )
            .with(
                &format!("{MEET_BASE}/spaces/s"),
                json!({"meetingCode": "abc-defg-hij"}),
            )
    }

    #[test]
    fn picks_notes_docs_only() {
        let ev = json!({"attachments": [
            {"fileId": "a", "title": "Planning - Notes by Gemini", "mimeType": GOOGLE_DOC_MIME},
            {"fileId": "b", "title": "Slides", "mimeType": GOOGLE_DOC_MIME},
            {"fileId": "c", "title": "notes.pdf", "mimeType": "application/pdf"},
            {"title": "Notes without id", "mimeType": GOOGLE_DOC_MIME}
        ]});
        assert_eq!(
            notes_attachments(&ev),
            vec![("a".to_string(), "Planning - Notes by Gemini".to_string())]
        );
        assert!(notes_attachments(&json!({})).is_empty());
    }

    #[tokio::test]
    async fn exports_smart_notes() {
        let api = base()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/smartNotes"),
                json!({"smartNotes": [{"name": "conferenceRecords/r/smartNotes/n",
                    "state": "FILE_GENERATED",
                    "docsDestination": {"document": "d1", "exportUri": "https://docs.google.com/d1"}}]}),
            )
            .with_text(&format!("{DRIVE_BASE}/files/d1/export"), "Summary\nDecisions");
        let out = notes(&api, &rec()).await.unwrap();
        assert_eq!(render_text(&out), "Summary\nDecisions");
        assert_eq!(out["notes"][0]["source"], "smartNotes");
    }

    #[tokio::test]
    async fn falls_back_to_calendar_attachment() {
        let api = base()
            .with(&format!("{MEET_BASE}/conferenceRecords/r/smartNotes"), json!({}))
            .with(
                &format!("{CALENDAR_BASE}/calendars/primary/events"),
                json!({"items": [{"hangoutLink": "https://meet.google.com/abc-defg-hij",
                    "start": {"dateTime": "2026-01-01T10:00:00Z"},
                    "attachments": [{"fileId": "d2", "title": "Notes by Gemini", "mimeType": GOOGLE_DOC_MIME}]}]}),
            )
            .with_text(&format!("{DRIVE_BASE}/files/d2/export"), "From calendar");
        let out = notes(&api, &rec()).await.unwrap();
        assert_eq!(render_text(&out), "From calendar");
        assert_eq!(out["notes"][0]["source"], "calendarAttachment");
    }

    #[tokio::test]
    async fn nothing_found_says_what_was_checked() {
        let api = base()
            .with(
                &format!("{MEET_BASE}/conferenceRecords/r/smartNotes"),
                json!({"smartNotes": [{"name": "n", "state": "STARTED"}]}),
            )
            .with(
                &format!("{CALENDAR_BASE}/calendars/primary/events"),
                json!({"items": []}),
            );
        let err = notes(&api, &rec()).await.unwrap_err().to_string();
        assert!(err.contains("No meeting notes"));
        assert!(err.contains("STARTED"));
        assert!(err.contains("no Calendar event"));
    }

    #[tokio::test]
    async fn smart_notes_error_still_tries_calendar() {
        let mut api = base();
        api.json.remove(&format!("{MEET_BASE}/spaces/s"));
        let err = notes(&api, &rec()).await.unwrap_err().to_string();
        assert!(err.contains("smart notes unavailable"));
        assert!(err.contains("meeting code unavailable"));
    }
}

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

//! Google Meet helpers: after a call, pull its attendees, transcript, notes
//! and recordings in one command each, joining the Meet REST API with
//! Calendar (event titles, notes attachments) and Drive (Doc exports,
//! recording files).

mod api;
mod attendees;
mod new;
mod notes;
mod recent;
mod recording;
mod records;
mod transcript;

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::formatter::OutputFormat;
use api::HttpApi;
use clap::{Arg, ArgMatches, Command};
use records::{parse_record_ref, RecordRef};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

const SCOPE_MEET_READ: &str = "https://www.googleapis.com/auth/meetings.space.readonly";
const SCOPE_MEET_CREATE: &str = "https://www.googleapis.com/auth/meetings.space.created";
const SCOPE_CALENDAR_READ: &str = "https://www.googleapis.com/auth/calendar.readonly";
const SCOPE_CALENDAR_EVENTS: &str = "https://www.googleapis.com/auth/calendar.events";
const SCOPE_DRIVE_READ: &str = "https://www.googleapis.com/auth/drive.readonly";

pub struct MeetHelper;

fn record_arg() -> Arg {
    Arg::new("record")
        .help("conferenceRecords/ID, a meeting code (abc-defg-hij), a Meet link, or \"latest\"")
        .required(true)
        .value_name("RECORD")
        .index(1)
}

impl Helper for MeetHelper {
    fn inject_commands(
        &self,
        mut cmd: Command,
        _doc: &crate::discovery::RestDescription,
    ) -> Command {
        cmd = cmd.subcommand(
            Command::new("+recent")
                .about("[Helper] List recent meetings with title, code, duration and head count")
                .arg(
                    Arg::new("days")
                        .long("days")
                        .help("How many days back to look")
                        .value_name("N")
                        .default_value("7")
                        .value_parser(clap::value_parser!(i64).range(1..=365)),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum number of meetings to list")
                        .value_name("N")
                        .default_value("20")
                        .value_parser(clap::value_parser!(u32).range(1..=500)),
                )
                .after_help(
                    "\
EXAMPLES:
  gws meet +recent
  gws meet +recent --days 30 --limit 5 --format json

TIPS:
  Newest first. Titles come from your primary Calendar, matched by meeting code.
  Use the record or meeting code with +attendees, +transcript, +notes, +recording.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+attendees")
                .about("[Helper] List who joined a meeting, when, and for how long")
                .arg(record_arg())
                .after_help(
                    "\
EXAMPLES:
  gws meet +attendees latest
  gws meet +attendees abc-defg-hij
  gws meet +attendees conferenceRecords/ID --format json

TIPS:
  Total time adds up every session, so rejoining is counted correctly.
  A meeting code resolves to its most recent conference.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+transcript")
                .about("[Helper] Print a meeting transcript as speaker-labelled text")
                .arg(record_arg())
                .after_help(
                    "\
EXAMPLES:
  gws meet +transcript latest
  gws meet +transcript abc-defg-hij --format json

TIPS:
  Output is plain text by default (--format text); use --format json for entries.
  Falls back to exporting the transcript Doc when the API holds no entries.
  Exits non-zero when the meeting has no transcript.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+notes")
                .about("[Helper] Print Gemini \"Take notes for me\" notes for a meeting")
                .arg(record_arg())
                .after_help(
                    "\
EXAMPLES:
  gws meet +notes latest
  gws meet +notes abc-defg-hij --format json

TIPS:
  Reads Meet smart notes first, then the notes Doc attached to the Calendar event.
  Exits non-zero when no notes exist.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+recording")
                .about("[Helper] List a meeting's recordings and optionally download them")
                .arg(record_arg())
                .arg(
                    Arg::new("download")
                        .long("download")
                        .help("Download generated recordings into this directory (relative to the current directory)")
                        .value_name("DIR"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws meet +recording latest
  gws meet +recording abc-defg-hij --download recordings

TIPS:
  Files already downloaded at full size are skipped.
  --download must be a relative path inside the current directory.",
                ),
        );
        cmd = cmd.subcommand(
            Command::new("+new")
                .about("[Helper] Create a meeting link, optionally with a Calendar event")
                .arg(
                    Arg::new("title")
                        .long("title")
                        .help("Also create a Calendar event with this title holding the link")
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("start")
                        .long("start")
                        .help("Event start (RFC 3339). Default: now")
                        .value_name("TIME")
                        .requires("title"),
                )
                .arg(
                    Arg::new("minutes")
                        .long("minutes")
                        .help("Event length in minutes. Default: 30")
                        .value_name("N")
                        .requires("title"),
                )
                .after_help(
                    "\
EXAMPLES:
  gws meet +new
  gws meet +new --title 'Design review'
  gws meet +new --title 'Standup' --start 2026-06-17T09:00:00-07:00 --minutes 15

TIPS:
  The event goes on your primary calendar with no guests; nobody is invited.
  Use --dry-run to see the requests without creating anything.",
                ),
        );
        cmd
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            let Some((name, sub)) = matches.subcommand() else {
                return Ok(false);
            };
            if !name.starts_with('+') {
                return Ok(false);
            }
            let fmt = sub.get_one::<String>("format").map(String::as_str);
            let now = chrono::Utc::now();
            match name {
                "+recent" => {
                    let days = *sub.get_one::<i64>("days").unwrap_or(&7);
                    let limit = *sub.get_one::<u32>("limit").unwrap_or(&20) as usize;
                    let api = connect(&[SCOPE_MEET_READ, SCOPE_CALENDAR_READ]).await?;
                    let out = recent::recent(&api, days, limit, now).await?;
                    print_out(&out, render_mode(fmt, false), None);
                }
                "+attendees" => {
                    let r = record_ref(sub)?;
                    let api = connect(&[SCOPE_MEET_READ]).await?;
                    let out = attendees::attendees(&api, &r, now).await?;
                    print_out(&out, render_mode(fmt, false), None);
                }
                "+transcript" => {
                    let r = record_ref(sub)?;
                    let api = connect(&[SCOPE_MEET_READ, SCOPE_DRIVE_READ]).await?;
                    let out = transcript::transcript(&api, &r).await?;
                    print_out(&out, render_mode(fmt, true), Some(transcript::render_text));
                }
                "+notes" => {
                    let r = record_ref(sub)?;
                    let api =
                        connect(&[SCOPE_MEET_READ, SCOPE_DRIVE_READ, SCOPE_CALENDAR_READ]).await?;
                    let out = notes::notes(&api, &r).await?;
                    print_out(&out, render_mode(fmt, true), Some(notes::render_text));
                }
                "+recording" => {
                    let r = record_ref(sub)?;
                    // Validate the output directory before any network call.
                    let dir = match sub.get_one::<String>("download") {
                        Some(d) => Some(validate_download_dir(d)?),
                        None => None,
                    };
                    let api = connect(&[SCOPE_MEET_READ, SCOPE_DRIVE_READ]).await?;
                    let mut out = recording::recordings(&api, &r).await?;
                    if let Some(dir) = dir {
                        if sub.get_flag("dry-run") {
                            annotate_plan(&mut out, &dir);
                        } else {
                            recording::download_all(&api, &mut out, &dir).await?;
                        }
                    }
                    print_out(&out, render_mode(fmt, false), None);
                }
                "+new" => {
                    let m = new::parse_new(
                        sub.get_one::<String>("title").map(String::as_str),
                        sub.get_one::<String>("start").map(String::as_str),
                        sub.get_one::<String>("minutes").map(String::as_str),
                        now,
                    )?;
                    let out = if sub.get_flag("dry-run") {
                        new::dry_run_plan(&m)
                    } else {
                        let mut scopes = vec![SCOPE_MEET_CREATE];
                        if m.title.is_some() {
                            scopes.push(SCOPE_CALENDAR_EVENTS);
                        }
                        let api = connect(&scopes).await?;
                        new::create(&api, &m).await?
                    };
                    print_out(&out, render_mode(fmt, false), None);
                }
                _ => return Ok(false),
            }
            Ok(true)
        })
    }

    fn extra_formats(&self) -> &'static [&'static str] {
        &["text"]
    }
}

fn record_ref(sub: &ArgMatches) -> Result<RecordRef, GwsError> {
    parse_record_ref(sub.get_one::<String>("record").map_or("", String::as_str))
}

fn validate_download_dir(dir: &str) -> Result<std::path::PathBuf, GwsError> {
    crate::validate::validate_safe_output_dir(dir)
        .map_err(|e| GwsError::Validation(e.to_string().replace("--output-dir", "--download")))
}

/// `--dry-run --download`: show where each file would go without fetching.
fn annotate_plan(out: &mut Value, dir: &std::path::Path) {
    if let Some(rows) = out["recordings"].as_array_mut() {
        for row in rows.iter_mut() {
            row["wouldSaveTo"] = match recording::plan_download(row, dir) {
                recording::Plan::Download { target, .. } => {
                    serde_json::json!(target.display().to_string())
                }
                recording::Plan::AlreadyThere(_) => serde_json::json!("(already downloaded)"),
                recording::Plan::Skip(why) => serde_json::json!(format!("(skipped: {why})")),
            };
        }
    }
}

async fn connect(scopes: &[&str]) -> Result<HttpApi, GwsError> {
    let token = auth::get_token(scopes)
        .await
        .map_err(|e| GwsError::Auth(format!("Meet auth failed: {e}")))?;
    HttpApi::new(token)
}

/// How to print a helper's result.
#[derive(Debug, PartialEq)]
enum Render {
    /// Plain text from the helper's own renderer.
    Text,
    Formatted(OutputFormat),
}

/// `--format text` (or no `--format` on a text-first helper) prints plain
/// text; anything else goes through the shared formatter. Listing helpers
/// default to a table, like `calendar +agenda`.
fn render_mode(fmt: Option<&str>, text_first: bool) -> Render {
    match fmt {
        None if text_first => Render::Text,
        None => Render::Formatted(OutputFormat::Table),
        Some(f) if f.eq_ignore_ascii_case("text") => {
            if text_first {
                Render::Text
            } else {
                Render::Formatted(OutputFormat::Table)
            }
        }
        Some(f) => Render::Formatted(OutputFormat::from_str(f)),
    }
}

fn render(out: &Value, mode: Render, text: Option<fn(&Value) -> String>) -> String {
    match (mode, text) {
        (Render::Text, Some(f)) => crate::output::sanitize_for_terminal(&f(out)),
        (Render::Text, None) => crate::formatter::format_value(out, &OutputFormat::Table),
        (Render::Formatted(f), _) => crate::formatter::format_value(out, &f),
    }
}

fn print_out(out: &Value, mode: Render, text: Option<fn(&Value) -> String>) {
    println!("{}", render(out, mode, text).trim_end());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cli() -> Command {
        let root = Command::new("gws")
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(clap::ArgAction::SetTrue)
                    .global(true),
            )
            .arg(Arg::new("format").long("format").global(true));
        MeetHelper.inject_commands(root, &crate::discovery::RestDescription::default())
    }

    #[test]
    fn registers_all_helpers() {
        let cmd = cli();
        for name in [
            "+recent",
            "+attendees",
            "+transcript",
            "+notes",
            "+recording",
            "+new",
        ] {
            assert!(cmd.find_subcommand(name).is_some(), "missing {name}");
        }
        assert!(matches!(
            crate::helpers::get_helper("meet").map(|h| h.extra_formats().to_vec()),
            Some(v) if v == vec!["text"]
        ));
    }

    #[test]
    fn recent_defaults_and_bounds() {
        let m = cli().try_get_matches_from(["gws", "+recent"]).unwrap();
        let sub = m.subcommand_matches("+recent").unwrap();
        assert_eq!(*sub.get_one::<i64>("days").unwrap(), 7);
        assert_eq!(*sub.get_one::<u32>("limit").unwrap(), 20);
        assert!(cli()
            .try_get_matches_from(["gws", "+recent", "--days", "0"])
            .is_err());
        assert!(cli()
            .try_get_matches_from(["gws", "+recent", "--limit", "501"])
            .is_err());
    }

    #[test]
    fn record_is_required() {
        for name in ["+attendees", "+transcript", "+notes", "+recording"] {
            assert!(cli().try_get_matches_from(["gws", name]).is_err(), "{name}");
        }
        let m = cli()
            .try_get_matches_from(["gws", "+transcript", "latest", "--format", "json"])
            .unwrap();
        let sub = m.subcommand_matches("+transcript").unwrap();
        assert_eq!(record_ref(sub).unwrap(), RecordRef::Latest);
        assert_eq!(sub.get_one::<String>("format").unwrap(), "json");
    }

    #[test]
    fn new_start_and_minutes_need_title() {
        assert!(cli()
            .try_get_matches_from(["gws", "+new", "--minutes", "10"])
            .is_err());
        assert!(cli()
            .try_get_matches_from(["gws", "+new", "--start", "x"])
            .is_err());
        assert!(cli()
            .try_get_matches_from(["gws", "+new", "--title", "t", "--minutes", "10"])
            .is_ok());
    }

    #[test]
    fn download_dir_is_validated() {
        assert!(validate_download_dir("../../.ssh").is_err());
        assert!(validate_download_dir("/tmp/x").is_err());
        let err = validate_download_dir("/tmp/x").unwrap_err().to_string();
        assert!(err.contains("--download"));
        assert!(validate_download_dir("recordings").is_ok());
    }

    #[test]
    fn render_modes() {
        assert_eq!(render_mode(None, true), Render::Text);
        assert_eq!(
            render_mode(None, false),
            Render::Formatted(OutputFormat::Table)
        );
        assert_eq!(render_mode(Some("TEXT"), true), Render::Text);
        assert_eq!(
            render_mode(Some("text"), false),
            Render::Formatted(OutputFormat::Table)
        );
        assert_eq!(
            render_mode(Some("json"), true),
            Render::Formatted(OutputFormat::Json)
        );
        assert_eq!(
            render_mode(Some("yaml"), false),
            Render::Formatted(OutputFormat::Yaml)
        );
    }

    #[test]
    fn renders_text_sanitized_or_formatted() {
        fn t(v: &Value) -> String {
            v["t"].as_str().unwrap().to_string()
        }
        let v = json!({"t": "hi\u{1b}[31m"});
        assert_eq!(render(&v, Render::Text, Some(t)), "hi[31m");
        assert!(render(&v, Render::Formatted(OutputFormat::Json), Some(t)).contains("\"t\""));
        assert!(render(&v, Render::Text, None).contains('t'));
    }

    #[test]
    fn dry_run_plan_annotates_rows() {
        let dir = tempfile::tempdir().unwrap();
        let mut out = json!({"recordings": [
            {"state": "FILE_GENERATED", "file": "f", "fileName": "a.mp4"},
            {"state": "STARTED"}
        ]});
        annotate_plan(&mut out, dir.path());
        assert!(out["recordings"][0]["wouldSaveTo"]
            .as_str()
            .unwrap()
            .ends_with("a.mp4"));
        assert_eq!(
            out["recordings"][1]["wouldSaveTo"],
            "(skipped: no Drive file)"
        );
    }
}

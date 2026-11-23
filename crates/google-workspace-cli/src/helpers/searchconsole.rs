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

//! `gws searchconsole +report`: clicks, impressions, CTR and position for a
//! Search Console property, grouped by query, page, country, device or date.

use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::formatter::OutputFormat;
use chrono::NaiveDate;
use clap::{Arg, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

const SEARCHCONSOLE_BASE: &str = "https://searchconsole.googleapis.com/webmasters/v3";
const SCOPE: &str = "https://www.googleapis.com/auth/webmasters.readonly";

/// Dimensions Search Console can group by.
pub(crate) const DIMENSIONS: &[&str] = &[
    "query",
    "page",
    "country",
    "device",
    "date",
    "searchAppearance",
];

/// Search types (`type` in the API request).
const SEARCH_TYPES: &[&str] = &["web", "image", "video", "news", "discover", "googleNews"];

/// Search Console keeps about 16 months of data.
const MAX_DAYS: i64 = 486;
/// The API's per-request row cap.
const MAX_ROWS: u32 = 25_000;

pub struct SearchConsoleHelper;

impl Helper for SearchConsoleHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+report")
                .about("[Helper] Clicks, impressions, CTR and position for a site")
                .arg(
                    Arg::new("site")
                        .help("Property: sc-domain:example.com or https://www.example.com/")
                        .required(true)
                        .value_name("SITE"),
                )
                .arg(
                    Arg::new("days")
                        .long("days")
                        .help("Days to report, ending today (default 28)")
                        .value_parser(clap::value_parser!(i64).range(1..=MAX_DAYS))
                        .default_value("28")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("start")
                        .long("start")
                        .help("Start date YYYY-MM-DD (overrides --days)")
                        .value_name("DATE"),
                )
                .arg(
                    Arg::new("end")
                        .long("end")
                        .help("End date YYYY-MM-DD (default today)")
                        .value_name("DATE"),
                )
                .arg(
                    Arg::new("by")
                        .long("by")
                        .help("Group by query, page, country, device, date or searchAppearance; comma-separate for several (default query)")
                        .default_value("query")
                        .value_name("DIMS"),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum rows (default 25, max 25000)")
                        .value_parser(clap::value_parser!(u32).range(1..=MAX_ROWS as i64))
                        .default_value("25")
                        .value_name("N"),
                )
                .arg(
                    Arg::new("type")
                        .long("type")
                        .help("Search type (default web)")
                        .value_parser(clap::builder::PossibleValuesParser::new(SEARCH_TYPES))
                        .default_value("web")
                        .value_name("TYPE"),
                )
                .arg(
                    Arg::new("final-only")
                        .long("final-only")
                        .help("Only finalised data (by default the last 2-3 days of fresh, still-changing data are included)")
                        .action(clap::ArgAction::SetTrue),
                )
                .after_help(
                    "\
EXAMPLES:
  gws searchconsole +report sc-domain:example.com
  gws searchconsole +report sc-domain:example.com --by page --days 90 --limit 50
  gws searchconsole +report https://www.example.com/ --by date --format csv
  gws searchconsole +report sc-domain:example.com --by query,country --format json

TIPS:
  Read-only. Default output is a table; --format csv|json|yaml also work.
  List your properties with: gws searchconsole sites list
  A newly added property can take a few days to show data.
  Needs the webmasters or webmasters.readonly scope.",
                ),
        )
    }

    fn handle<'a>(
        &'a self,
        _doc: &'a crate::discovery::RestDescription,
        matches: &'a ArgMatches,
        _sanitize_config: &'a crate::helpers::modelarmor::SanitizeConfig,
    ) -> Pin<Box<dyn Future<Output = Result<bool, GwsError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(m) = matches.subcommand_matches("+report") {
                handle_report(m).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

/// Parsed, validated `+report` options.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReportOptions {
    pub site: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub dimensions: Vec<String>,
    pub limit: u32,
    pub search_type: String,
    pub data_state: &'static str,
}

/// Output format for helpers whose natural form is a table.
pub(crate) fn table_by_default(matches: &ArgMatches) -> OutputFormat {
    match matches.get_one::<String>("format") {
        Some(f) => OutputFormat::parse(f).unwrap_or_else(|unknown| {
            eprintln!(
                "warning: unknown output format '{unknown}'; using table (valid: table, csv, json, yaml)"
            );
            OutputFormat::Table
        }),
        None => OutputFormat::Table,
    }
}

/// Validates a Search Console property identifier.
///
/// Accepts `sc-domain:<domain>` (domain property) or an `http(s)://` URL
/// prefix property. A bare domain is rejected with a suggestion, since it is
/// ambiguous between the two kinds.
pub(crate) fn validate_site(site: &str) -> Result<String, GwsError> {
    crate::validate::reject_dangerous_chars(site, "site")?;
    let site = site.trim();
    if let Some(domain) = site.strip_prefix("sc-domain:") {
        let ok = !domain.is_empty()
            && domain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            && !domain.starts_with('.')
            && !domain.contains("..");
        if !ok {
            return Err(GwsError::Validation(format!(
                "Invalid domain property '{site}': expected sc-domain:example.com"
            )));
        }
        return Ok(site.to_string());
    }
    if site.starts_with("https://") || site.starts_with("http://") {
        if site.contains(char::is_whitespace) {
            return Err(GwsError::Validation(format!(
                "Invalid URL-prefix property '{site}'"
            )));
        }
        return Ok(site.to_string());
    }
    Err(GwsError::Validation(format!(
        "Unrecognised Search Console property '{site}'. Use sc-domain:{site} for a domain \
         property or https://{site}/ for a URL-prefix property (see: gws searchconsole sites list)"
    )))
}

/// Parses a comma-separated dimension list against an allowlist.
pub(crate) fn parse_dimensions(raw: &str, allowed: &[&str]) -> Result<Vec<String>, GwsError> {
    let mut dims: Vec<String> = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let Some(canonical) = allowed.iter().find(|a| a.eq_ignore_ascii_case(part)) else {
            return Err(GwsError::Validation(format!(
                "Unknown --by value '{part}'. Valid: {}",
                allowed.join(", ")
            )));
        };
        if !dims.iter().any(|d| d == canonical) {
            dims.push(canonical.to_string());
        }
    }
    if dims.is_empty() {
        return Err(GwsError::Validation(
            "--by needs at least one dimension".to_string(),
        ));
    }
    Ok(dims)
}

/// Parses a `YYYY-MM-DD` flag value.
pub(crate) fn parse_date(value: &str, flag: &str) -> Result<NaiveDate, GwsError> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| {
        GwsError::Validation(format!(
            "--{flag} must be a date like 2026-11-01, got '{value}'"
        ))
    })
}

/// Resolves the reporting window: `days` days ending `end` (default
/// `today`), unless `start` is given explicitly. Both ends are inclusive.
pub(crate) fn date_window(
    today: NaiveDate,
    days: i64,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<(NaiveDate, NaiveDate), GwsError> {
    let end_date = match end {
        Some(e) => parse_date(e, "end")?,
        None => today,
    };
    let start_date = match start {
        Some(s) => parse_date(s, "start")?,
        None => end_date - chrono::Duration::days(days - 1),
    };
    if start_date > end_date {
        return Err(GwsError::Validation(format!(
            "--start ({start_date}) is after --end ({end_date})"
        )));
    }
    Ok((start_date, end_date))
}

fn parse_options(matches: &ArgMatches, today: NaiveDate) -> Result<ReportOptions, GwsError> {
    let site = validate_site(matches.get_one::<String>("site").expect("required"))?;
    let days = *matches.get_one::<i64>("days").unwrap_or(&28);
    let (start, end) = date_window(
        today,
        days,
        matches.get_one::<String>("start").map(String::as_str),
        matches.get_one::<String>("end").map(String::as_str),
    )?;
    let dimensions = parse_dimensions(
        matches
            .get_one::<String>("by")
            .map(String::as_str)
            .unwrap_or("query"),
        DIMENSIONS,
    )?;
    Ok(ReportOptions {
        site,
        start,
        end,
        dimensions,
        limit: *matches.get_one::<u32>("limit").unwrap_or(&25),
        search_type: matches
            .get_one::<String>("type")
            .cloned()
            .unwrap_or_else(|| "web".to_string()),
        data_state: if matches.get_flag("final-only") {
            "final"
        } else {
            "all"
        },
    })
}

/// Request body for `searchAnalytics.query`. With no dimensions it returns
/// one row of totals.
pub(crate) fn build_query_body(opts: &ReportOptions, dimensions: &[String], limit: u32) -> Value {
    let mut body = json!({
        "startDate": opts.start.to_string(),
        "endDate": opts.end.to_string(),
        "type": opts.search_type,
        "dataState": opts.data_state,
        "rowLimit": limit,
    });
    if !dimensions.is_empty() {
        body["dimensions"] = json!(dimensions);
    }
    body
}

pub(crate) fn query_url(site: &str) -> String {
    format!(
        "{SEARCHCONSOLE_BASE}/sites/{}/searchAnalytics/query",
        crate::validate::encode_path_segment(site)
    )
}

fn num(row: &Value, key: &str) -> f64 {
    row.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

/// Turns a `searchAnalytics.query` response into ordered columns and rows.
///
/// `human` renders CTR as a percentage string and position to one decimal,
/// for table and CSV; otherwise CTR stays a 0-1 ratio for JSON consumers.
pub(crate) fn build_rows(
    resp: &Value,
    dimensions: &[String],
    human: bool,
) -> (Vec<String>, Vec<Vec<Value>>) {
    let mut columns: Vec<String> = dimensions.to_vec();
    columns.extend(
        ["clicks", "impressions", "ctr", "position"]
            .iter()
            .map(|s| s.to_string()),
    );
    let rows = resp
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let keys = row.get("keys").and_then(Value::as_array);
                    let mut out: Vec<Value> = (0..dimensions.len())
                        .map(|i| {
                            keys.and_then(|k| k.get(i))
                                .cloned()
                                .unwrap_or(Value::String(String::new()))
                        })
                        .collect();
                    out.extend(metric_cells(row, human));
                    out
                })
                .collect()
        })
        .unwrap_or_default();
    (columns, rows)
}

fn metric_cells(row: &Value, human: bool) -> Vec<Value> {
    let clicks = num(row, "clicks");
    let impressions = num(row, "impressions");
    let ctr = num(row, "ctr");
    let position = num(row, "position");
    if human {
        vec![
            json!(clicks as u64),
            json!(impressions as u64),
            json!(format!("{:.2}%", ctr * 100.0)),
            json!(format!("{position:.1}")),
        ]
    } else {
        vec![
            json!(clicks as u64),
            json!(impressions as u64),
            json!(round(ctr, 4)),
            json!(round(position, 2)),
        ]
    }
}

/// Totals from a dimensionless query, as a JSON object (None when empty).
pub(crate) fn totals(resp: &Value) -> Option<Value> {
    let row = resp.get("rows")?.as_array()?.first()?;
    Some(json!({
        "clicks": num(row, "clicks") as u64,
        "impressions": num(row, "impressions") as u64,
        "ctr": round(num(row, "ctr"), 4),
        "position": round(num(row, "position"), 2),
    }))
}

pub(crate) fn title_line(opts: &ReportOptions, totals: Option<&Value>, shown: usize) -> String {
    let days = (opts.end - opts.start).num_days() + 1;
    let mut t = format!(
        "{} · {} → {} ({days} days) · {} search · by {}",
        opts.site,
        opts.start,
        opts.end,
        opts.search_type,
        opts.dimensions.join(", ")
    );
    if shown > 0 {
        t.push_str(&format!(" · top {shown}"));
    }
    if let Some(tot) = totals {
        t.push_str(&format!(
            "\nTotals: {} clicks · {} impressions · CTR {:.2}% · avg position {:.1}",
            tot["clicks"],
            tot["impressions"],
            tot["ctr"].as_f64().unwrap_or(0.0) * 100.0,
            tot["position"].as_f64().unwrap_or(0.0)
        ));
    }
    t
}

pub(crate) fn empty_note(opts: &ReportOptions) -> String {
    format!(
        "No data in range: Search Console has no {} search data for {} between {} and {}. \
         A newly added or newly verified property can take a few days to show data.",
        opts.search_type, opts.site, opts.start, opts.end
    )
}

async fn handle_report(matches: &ArgMatches) -> Result<(), GwsError> {
    let today = chrono::Local::now().date_naive();
    let opts = parse_options(matches, today)?;
    let format = table_by_default(matches);
    let url = query_url(&opts.site);
    let body = build_query_body(&opts, &opts.dimensions, opts.limit);
    let totals_body = build_query_body(&opts, &[], 1);

    if matches.get_flag("dry-run") {
        let out = json!({
            "dry_run": true,
            "requests": [
                {"method": "POST", "url": url, "body": body},
                {"method": "POST", "url": url, "body": totals_body},
            ],
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return Ok(());
    }

    let token = auth::get_token(&[SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Search Console auth failed: {e}")))?;
    let client = crate::client::shared_client()?;

    let (resp, totals_resp) = tokio::try_join!(
        crate::executor::fetch_json(
            || client.post(&url).bearer_auth(&token).json(&body),
            "Search Console query",
        ),
        crate::executor::fetch_json(
            || client.post(&url).bearer_auth(&token).json(&totals_body),
            "Search Console totals query",
        ),
    )?;

    let human = matches!(format, OutputFormat::Table | OutputFormat::Csv);
    let (columns, rows) = build_rows(&resp, &opts.dimensions, human);
    let totals = totals(&totals_resp);
    let meta = json!({
        "site": opts.site,
        "startDate": opts.start.to_string(),
        "endDate": opts.end.to_string(),
        "type": opts.search_type,
        "dataState": opts.data_state,
        "dimensions": opts.dimensions,
        "totals": totals,
    });
    let title = title_line(&opts, totals.as_ref(), rows.len());
    let rendered = crate::formatter::render_report(
        &columns,
        &rows,
        &meta,
        Some(&title),
        &empty_note(&opts),
        &format,
    );
    print!("{}", rendered.stdout);
    if let Some(n) = rendered.notice {
        eprintln!("{n}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn cmd() -> Command {
        let root = Command::new("gws").arg(
            Arg::new("format")
                .long("format")
                .global(true)
                .value_name("FORMAT"),
        );
        let root = root.arg(
            Arg::new("dry-run")
                .long("dry-run")
                .global(true)
                .action(clap::ArgAction::SetTrue),
        );
        SearchConsoleHelper.inject_commands(root, &Default::default())
    }

    fn report_matches(args: &[&str]) -> ArgMatches {
        let mut argv = vec!["gws", "+report"];
        argv.extend_from_slice(args);
        let m = cmd().try_get_matches_from(argv).unwrap();
        m.subcommand_matches("+report").unwrap().clone()
    }

    #[test]
    fn validate_site_accepts_domain_and_url_properties() {
        assert_eq!(
            validate_site("sc-domain:mbprep.app").unwrap(),
            "sc-domain:mbprep.app"
        );
        assert!(validate_site("https://www.example.com/").is_ok());
        assert!(validate_site("http://example.com/blog/").is_ok());
    }

    #[test]
    fn validate_site_rejects_bad_input() {
        let err = validate_site("mbprep.app").unwrap_err().to_string();
        assert!(err.contains("sc-domain:mbprep.app"), "{err}");
        assert!(validate_site("sc-domain:").is_err());
        assert!(validate_site("sc-domain:../etc").is_err());
        assert!(validate_site("sc-domain:a/b").is_err());
        assert!(validate_site("https://a.com/\u{0007}").is_err());
        assert!(validate_site("https://a.com/ b").is_err());
    }

    #[test]
    fn query_url_encodes_site() {
        assert_eq!(
            query_url("sc-domain:mbprep.app"),
            "https://searchconsole.googleapis.com/webmasters/v3/sites/sc%2Ddomain%3Ambprep%2Eapp/searchAnalytics/query"
        );
        assert!(query_url("https://x.com/a?b").contains("https%3A%2F%2Fx%2Ecom%2Fa%3Fb"));
    }

    #[test]
    fn parse_dimensions_allowlist_and_dedup() {
        assert_eq!(
            parse_dimensions("query, PAGE,query", DIMENSIONS).unwrap(),
            vec!["query", "page"]
        );
        assert_eq!(
            parse_dimensions("searchappearance", DIMENSIONS).unwrap(),
            vec!["searchAppearance"]
        );
        assert!(parse_dimensions("keyword", DIMENSIONS).is_err());
        assert!(parse_dimensions(" , ", DIMENSIONS).is_err());
    }

    #[test]
    fn date_window_counts_inclusive_days() {
        let today = d("2026-11-23");
        assert_eq!(
            date_window(today, 28, None, None).unwrap(),
            (d("2026-10-27"), d("2026-11-23"))
        );
        assert_eq!(date_window(today, 1, None, None).unwrap(), (today, today));
        assert_eq!(
            date_window(today, 7, None, Some("2026-11-10")).unwrap(),
            (d("2026-11-04"), d("2026-11-10"))
        );
        assert_eq!(
            date_window(today, 28, Some("2026-11-01"), None).unwrap(),
            (d("2026-11-01"), today)
        );
    }

    #[test]
    fn date_window_rejects_bad_dates() {
        let today = d("2026-11-23");
        assert!(date_window(today, 28, Some("2026-13-01"), None).is_err());
        assert!(date_window(today, 28, Some("yesterday"), None).is_err());
        assert!(date_window(today, 28, Some("2026-11-20"), Some("2026-11-01")).is_err());
    }

    #[test]
    fn parse_options_defaults() {
        let m = report_matches(&["sc-domain:mbprep.app"]);
        let o = parse_options(&m, d("2026-11-23")).unwrap();
        assert_eq!(o.dimensions, vec!["query"]);
        assert_eq!(o.limit, 25);
        assert_eq!(o.search_type, "web");
        assert_eq!(o.data_state, "all");
        assert_eq!(o.start, d("2026-10-27"));
    }

    #[test]
    fn parse_options_flags() {
        let m = report_matches(&[
            "sc-domain:mbprep.app",
            "--by",
            "page,device",
            "--days",
            "7",
            "--limit",
            "100",
            "--type",
            "image",
            "--final-only",
        ]);
        let o = parse_options(&m, d("2026-11-23")).unwrap();
        assert_eq!(o.dimensions, vec!["page", "device"]);
        assert_eq!(o.limit, 100);
        assert_eq!(o.search_type, "image");
        assert_eq!(o.data_state, "final");
        assert_eq!(o.start, d("2026-11-17"));
    }

    #[test]
    fn clap_rejects_out_of_range_values() {
        let c = cmd();
        assert!(c
            .clone()
            .try_get_matches_from(["gws", "+report", "sc-domain:a.com", "--days", "0"])
            .is_err());
        assert!(c
            .clone()
            .try_get_matches_from(["gws", "+report", "sc-domain:a.com", "--limit", "25001"])
            .is_err());
        assert!(c
            .try_get_matches_from(["gws", "+report", "sc-domain:a.com", "--type", "shopping"])
            .is_err());
    }

    #[test]
    fn build_query_body_shapes_request() {
        let m = report_matches(&["sc-domain:mbprep.app", "--by", "country"]);
        let o = parse_options(&m, d("2026-11-23")).unwrap();
        let b = build_query_body(&o, &o.dimensions, o.limit);
        assert_eq!(b["startDate"], "2026-10-27");
        assert_eq!(b["endDate"], "2026-11-23");
        assert_eq!(b["dimensions"], json!(["country"]));
        assert_eq!(b["rowLimit"], 25);
        assert_eq!(b["dataState"], "all");
        let t = build_query_body(&o, &[], 1);
        assert!(t.get("dimensions").is_none());
    }

    fn sample_resp() -> Value {
        json!({"rows": [
            {"keys": ["mbprep", "nga"], "clicks": 12.0, "impressions": 300.0, "ctr": 0.04, "position": 3.456},
            {"keys": ["mb prep app", "gbr"], "clicks": 1.0, "impressions": 10.0, "ctr": 0.1, "position": 8.0}
        ]})
    }

    #[test]
    fn build_rows_machine_values() {
        let dims = vec!["query".to_string(), "country".to_string()];
        let (cols, rows) = build_rows(&sample_resp(), &dims, false);
        assert_eq!(
            cols,
            vec![
                "query",
                "country",
                "clicks",
                "impressions",
                "ctr",
                "position"
            ]
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "mbprep");
        assert_eq!(rows[0][1], "nga");
        assert_eq!(rows[0][2], 12);
        assert_eq!(rows[0][4], 0.04);
        assert_eq!(rows[0][5], 3.46);
    }

    #[test]
    fn build_rows_human_values() {
        let dims = vec!["query".to_string(), "country".to_string()];
        let (_, rows) = build_rows(&sample_resp(), &dims, true);
        assert_eq!(rows[0][4], "4.00%");
        assert_eq!(rows[0][5], "3.5");
    }

    #[test]
    fn build_rows_empty_response() {
        let (cols, rows) = build_rows(&json!({}), &["query".to_string()], true);
        assert_eq!(cols.len(), 5);
        assert!(rows.is_empty());
    }

    #[test]
    fn totals_and_title() {
        let m = report_matches(&["sc-domain:mbprep.app"]);
        let o = parse_options(&m, d("2026-11-23")).unwrap();
        assert!(totals(&json!({})).is_none());
        let t = totals(
            &json!({"rows":[{"clicks": 5.0, "impressions": 50.0, "ctr": 0.1, "position": 4.25}]}),
        )
        .unwrap();
        assert_eq!(t["clicks"], 5);
        let title = title_line(&o, Some(&t), 3);
        assert!(title.contains("sc-domain:mbprep.app · 2026-10-27 → 2026-11-23 (28 days)"));
        assert!(title.contains("top 3"));
        assert!(title.contains("Totals: 5 clicks · 50 impressions · CTR 10.00% · avg position 4.2"));
        let note = empty_note(&o);
        assert!(note.starts_with("No data in range"));
        assert!(note.contains("sc-domain:mbprep.app"));
    }

    #[test]
    fn empty_report_renders_note_in_every_format() {
        let m = report_matches(&["sc-domain:mbprep.app"]);
        let o = parse_options(&m, d("2026-11-23")).unwrap();
        let (cols, rows) = build_rows(&json!({}), &o.dimensions, true);
        let table = crate::formatter::render_report(
            &cols,
            &rows,
            &json!({}),
            Some(&title_line(&o, None, 0)),
            &empty_note(&o),
            &OutputFormat::Table,
        );
        assert!(table.stdout.contains("No data in range"));
        assert!(!table.stdout.contains('─'));
    }

    #[test]
    fn table_by_default_respects_format_flag() {
        let m = report_matches(&["sc-domain:a.com"]);
        assert_eq!(table_by_default(&m), OutputFormat::Table);
        let m = report_matches(&["sc-domain:a.com", "--format", "csv"]);
        assert_eq!(table_by_default(&m), OutputFormat::Csv);
    }
}

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

//! Google Analytics 4 helpers:
//! - `gws analytics +report <property>`: an audience report (users, sessions,
//!   views, engagement) grouped by a dimension.
//! - `gws analytics +properties`: the accounts and GA4 properties you can read.

use super::searchconsole::{date_window, parse_dimensions, table_by_default};
use super::Helper;
use crate::auth;
use crate::error::GwsError;
use crate::formatter::OutputFormat;
use chrono::NaiveDate;
use clap::{Arg, ArgMatches, Command};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

const DATA_BASE: &str = "https://analyticsdata.googleapis.com/v1beta";
const ADMIN_BASE: &str = "https://analyticsadmin.googleapis.com/v1beta";
const SCOPE: &str = "https://www.googleapis.com/auth/analytics.readonly";

/// Dimensions `+report --by` accepts.
pub(crate) const DIMENSIONS: &[&str] = &[
    "country",
    "city",
    "region",
    "deviceCategory",
    "operatingSystem",
    "browser",
    "language",
    "sessionSource",
    "sessionMedium",
    "sessionSourceMedium",
    "sessionDefaultChannelGroup",
    "sessionCampaignName",
    "firstUserSource",
    "pagePath",
    "pageTitle",
    "landingPage",
    "hostName",
    "eventName",
    "newVsReturning",
    "date",
    "userAgeBracket",
    "userGender",
];

const DEFAULT_METRICS: &str = "activeUsers,newUsers,sessions,screenPageViews,engagementRate";
/// The Data API accepts at most 10 metrics per request.
const MAX_METRICS: usize = 10;
const MAX_DAYS: i64 = 3650;

pub struct AnalyticsHelper;

impl Helper for AnalyticsHelper {
    fn inject_commands(&self, cmd: Command, _doc: &crate::discovery::RestDescription) -> Command {
        cmd.subcommand(
            Command::new("+report")
                .about("[Helper] GA4 audience report: users, sessions and views by a dimension")
                .arg(
                    Arg::new("property")
                        .help("GA4 property ID (123456789 or properties/123456789)")
                        .required(true)
                        .value_name("PROPERTY"),
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
                        .help("Dimension(s) to group by, comma-separated (default country). See --help for the list")
                        .default_value("country")
                        .value_name("DIMS"),
                )
                .arg(
                    Arg::new("metrics")
                        .long("metrics")
                        .help("Comma-separated GA4 metrics (default activeUsers,newUsers,sessions,screenPageViews,engagementRate)")
                        .default_value(DEFAULT_METRICS)
                        .value_name("LIST"),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .help("Maximum rows (default 25)")
                        .value_parser(clap::value_parser!(u32).range(1..=250_000))
                        .default_value("25")
                        .value_name("N"),
                )
                .after_help(
                    "\
DIMENSIONS (--by):
  country, city, region, deviceCategory, operatingSystem, browser, language,
  sessionSource, sessionMedium, sessionSourceMedium, sessionDefaultChannelGroup,
  sessionCampaignName, firstUserSource, pagePath, pageTitle, landingPage,
  hostName, eventName, newVsReturning, date, userAgeBracket, userGender

EXAMPLES:
  gws analytics +report 123456789
  gws analytics +report 123456789 --by sessionSource --days 7
  gws analytics +report 123456789 --by pagePath --metrics screenPageViews,averageSessionDuration
  gws analytics +report 123456789 --by date --format csv
  gws analytics +report 123456789 --by userAgeBracket,userGender

TIPS:
  Read-only. Rows are sorted by the first metric, or by date when grouping by date.
  Find property IDs with: gws analytics +properties
  Age and gender need Google signals enabled, and GA4 hides small groups.
  Needs the analytics.readonly (or analytics) scope.",
                ),
        )
        .subcommand(
            Command::new("+properties")
                .about("[Helper] List the Analytics accounts and GA4 properties you can access")
                .after_help(
                    "\
EXAMPLES:
  gws analytics +properties
  gws analytics +properties --format json

TIPS:
  Read-only. Use a property ID from here with: gws analytics +report <id>",
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
            if let Some(m) = matches.subcommand_matches("+properties") {
                handle_properties(m).await?;
                return Ok(true);
            }
            Ok(false)
        })
    }
}

/// Normalises a property argument to `properties/<digits>`.
pub(crate) fn normalize_property(raw: &str) -> Result<String, GwsError> {
    crate::validate::reject_dangerous_chars(raw, "property")?;
    let id = raw.trim().strip_prefix("properties/").unwrap_or(raw.trim());
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(GwsError::Validation(format!(
            "Invalid GA4 property '{raw}': expected a numeric ID like 123456789 \
             (find yours with: gws analytics +properties). A G-XXXX measurement ID is not a property ID."
        )));
    }
    Ok(format!("properties/{id}"))
}

/// Parses `--metrics` into a de-duplicated list of API metric names.
pub(crate) fn parse_metrics(raw: &str) -> Result<Vec<String>, GwsError> {
    let mut metrics: Vec<String> = Vec::new();
    for m in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let valid = m.len() <= 100
            && m.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':');
        if !valid {
            return Err(GwsError::Validation(format!(
                "Invalid metric '{m}': metric names are letters, digits, '_' and ':' (e.g. activeUsers, customEvent:score)"
            )));
        }
        if !metrics.iter().any(|x| x == m) {
            metrics.push(m.to_string());
        }
    }
    if metrics.is_empty() {
        return Err(GwsError::Validation(
            "--metrics needs at least one metric".to_string(),
        ));
    }
    if metrics.len() > MAX_METRICS {
        return Err(GwsError::Validation(format!(
            "--metrics has {} metrics; the Analytics Data API allows at most {MAX_METRICS}",
            metrics.len()
        )));
    }
    Ok(metrics)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReportOptions {
    pub property: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
    pub limit: u32,
}

fn parse_options(matches: &ArgMatches, today: NaiveDate) -> Result<ReportOptions, GwsError> {
    let property = normalize_property(matches.get_one::<String>("property").expect("required"))?;
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
            .unwrap_or("country"),
        DIMENSIONS,
    )?;
    let metrics = parse_metrics(
        matches
            .get_one::<String>("metrics")
            .map(String::as_str)
            .unwrap_or(DEFAULT_METRICS),
    )?;
    Ok(ReportOptions {
        property,
        start,
        end,
        dimensions,
        metrics,
        limit: *matches.get_one::<u32>("limit").unwrap_or(&25),
    })
}

/// Request body for `properties.runReport`.
pub(crate) fn build_report_body(opts: &ReportOptions) -> Value {
    let order_bys = if opts.dimensions.iter().any(|d| d == "date") {
        json!([{"dimension": {"dimensionName": "date"}, "desc": false}])
    } else {
        json!([{"metric": {"metricName": opts.metrics[0]}, "desc": true}])
    };
    json!({
        "dateRanges": [{"startDate": opts.start.to_string(), "endDate": opts.end.to_string()}],
        "dimensions": opts.dimensions.iter().map(|d| json!({"name": d})).collect::<Vec<_>>(),
        "metrics": opts.metrics.iter().map(|m| json!({"name": m})).collect::<Vec<_>>(),
        "orderBys": order_bys,
        "limit": opts.limit,
        "metricAggregations": ["TOTAL"],
        "keepEmptyRows": false,
    })
}

pub(crate) fn report_url(property: &str) -> String {
    // property is validated as properties/<digits>; keep the slash.
    format!(
        "{DATA_BASE}/{}:runReport",
        crate::validate::encode_path_preserving_slashes(property)
    )
}

/// GA4 `date` values come back as YYYYMMDD.
fn pretty_date(v: &str) -> String {
    if v.len() == 8 && v.chars().all(|c| c.is_ascii_digit()) {
        format!("{}-{}-{}", &v[0..4], &v[4..6], &v[6..8])
    } else {
        v.to_string()
    }
}

/// Formats one metric value by its declared type.
fn metric_cell(raw: &str, metric_type: &str, name: &str, human: bool) -> Value {
    let parsed = raw.parse::<f64>().ok();
    let Some(v) = parsed else {
        return Value::String(raw.to_string());
    };
    let is_rate = name.ends_with("Rate") || name.ends_with("Percentage");
    match metric_type {
        "TYPE_INTEGER" => json!(v as i64),
        _ if is_rate && human => json!(format!("{:.1}%", v * 100.0)),
        "TYPE_SECONDS" if human => json!(format!("{v:.1}s")),
        _ if human => json!(format!("{v:.2}")),
        _ => json!((v * 10_000.0).round() / 10_000.0),
    }
}

fn metric_types(resp: &Value) -> Vec<(String, String)> {
    resp.get("metricHeaders")
        .and_then(Value::as_array)
        .map(|h| {
            h.iter()
                .map(|m| {
                    (
                        m.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        m.get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("TYPE_FLOAT")
                            .to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Turns a `runReport` response into ordered columns and rows.
pub(crate) fn build_rows(
    resp: &Value,
    opts: &ReportOptions,
    human: bool,
) -> (Vec<String>, Vec<Vec<Value>>) {
    let mut columns: Vec<String> = opts.dimensions.clone();
    columns.extend(opts.metrics.iter().cloned());
    let types = metric_types(resp);
    let type_of = |i: usize| {
        types
            .get(i)
            .map(|(_, t)| t.as_str())
            .unwrap_or("TYPE_FLOAT")
    };
    let rows = resp
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let dims = row.get("dimensionValues").and_then(Value::as_array);
                    let mets = row.get("metricValues").and_then(Value::as_array);
                    let mut out: Vec<Value> = opts
                        .dimensions
                        .iter()
                        .enumerate()
                        .map(|(i, name)| {
                            let v = dims
                                .and_then(|d| d.get(i))
                                .and_then(|d| d.get("value"))
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let v = if name == "date" {
                                pretty_date(v)
                            } else {
                                v.to_string()
                            };
                            Value::String(v)
                        })
                        .collect();
                    for (i, name) in opts.metrics.iter().enumerate() {
                        let raw = mets
                            .and_then(|m| m.get(i))
                            .and_then(|m| m.get("value"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        out.push(metric_cell(raw, type_of(i), name, human));
                    }
                    out
                })
                .collect()
        })
        .unwrap_or_default();
    (columns, rows)
}

/// Totals from `metricAggregations: TOTAL`, keyed by metric name.
pub(crate) fn totals(resp: &Value, opts: &ReportOptions) -> Option<Value> {
    let row = resp.get("totals")?.as_array()?.first()?;
    let mets = row.get("metricValues")?.as_array()?;
    let types = metric_types(resp);
    let mut obj = serde_json::Map::new();
    for (i, name) in opts.metrics.iter().enumerate() {
        let raw = mets
            .get(i)
            .and_then(|m| m.get("value"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let t = types
            .get(i)
            .map(|(_, t)| t.as_str())
            .unwrap_or("TYPE_FLOAT");
        obj.insert(name.clone(), metric_cell(raw, t, name, false));
    }
    Some(Value::Object(obj))
}

pub(crate) fn title_line(opts: &ReportOptions, totals: Option<&Value>, total_rows: u64) -> String {
    let days = (opts.end - opts.start).num_days() + 1;
    let mut t = format!(
        "{} · {} → {} ({days} days) · by {}",
        opts.property,
        opts.start,
        opts.end,
        opts.dimensions.join(", ")
    );
    if total_rows > 0 {
        t.push_str(&format!(" · {total_rows} rows in total"));
    }
    if let Some(Value::Object(tot)) = totals {
        let parts: Vec<String> = opts
            .metrics
            .iter()
            .filter_map(|m| tot.get(m).map(|v| format!("{m} {}", total_cell(m, v))))
            .collect();
        if !parts.is_empty() {
            t.push_str(&format!("\nTotals: {}", parts.join(" · ")));
        }
    }
    t
}

fn total_cell(name: &str, v: &Value) -> String {
    match v.as_f64() {
        Some(f) if name.ends_with("Rate") || name.ends_with("Percentage") => {
            format!("{:.1}%", f * 100.0)
        }
        Some(f) if f.fract() != 0.0 => format!("{f:.2}"),
        _ => v.to_string(),
    }
}

pub(crate) fn empty_note(opts: &ReportOptions) -> String {
    format!(
        "No data in range: {} recorded nothing between {} and {}. If the site has no GA4 \
         tag yet (the Google tag with the property's G- measurement ID), the property \
         collects no data; new data also takes up to 24-48 hours to appear.",
        opts.property, opts.start, opts.end
    )
}

/// The Data API needs `analytics.readonly` or `analytics`; `analytics.edit`
/// (which `+properties` can use) is not enough. Say so on a scope 403.
pub(crate) fn explain_scope_error(err: GwsError) -> GwsError {
    match err {
        GwsError::Api {
            code: 403,
            message,
            reason,
            enable_url,
        } if message.contains("insufficient authentication scopes") => GwsError::Api {
            code: 403,
            message: format!(
                "{message} The GA4 Data API needs the analytics.readonly (or analytics) scope; \
                 analytics.edit only covers the Admin API. Sign in again with it added, e.g. \
                 gws auth login --scopes <your current scopes>,{SCOPE} \
                 (gws auth status lists the current ones)."
            ),
            reason,
            enable_url,
        },
        other => other,
    }
}

async fn handle_report(matches: &ArgMatches) -> Result<(), GwsError> {
    let today = chrono::Local::now().date_naive();
    let opts = parse_options(matches, today)?;
    let format = table_by_default(matches);
    let url = report_url(&opts.property);
    let body = build_report_body(&opts);

    if matches.get_flag("dry-run") {
        let out = json!({"dry_run": true, "method": "POST", "url": url, "body": body});
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return Ok(());
    }

    let token = auth::get_token(&[SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Analytics auth failed: {e}")))?;
    let client = crate::client::shared_client()?;
    let resp = crate::executor::fetch_json(
        || client.post(&url).bearer_auth(&token).json(&body),
        "Analytics runReport",
    )
    .await
    .map_err(explain_scope_error)?;

    let human = matches!(format, OutputFormat::Table | OutputFormat::Csv);
    let (columns, rows) = build_rows(&resp, &opts, human);
    let totals = if rows.is_empty() {
        None
    } else {
        totals(&resp, &opts)
    };
    let row_count = resp.get("rowCount").and_then(Value::as_u64).unwrap_or(0);
    let meta = json!({
        "property": opts.property,
        "startDate": opts.start.to_string(),
        "endDate": opts.end.to_string(),
        "dimensions": opts.dimensions,
        "metrics": opts.metrics,
        "rowCount": row_count,
        "totals": totals,
    });
    let title = title_line(&opts, totals.as_ref(), row_count);
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

/// Flattens `accountSummaries` into one row per property (or per account
/// with no properties).
pub(crate) fn property_rows(summaries: &[Value]) -> Vec<Vec<Value>> {
    let s = |v: &Value, k: &str| -> String {
        v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
    };
    let mut rows = Vec::new();
    for acct in summaries {
        let account = s(acct, "account");
        let account_id = account
            .strip_prefix("accounts/")
            .unwrap_or(&account)
            .to_string();
        let account_name = s(acct, "displayName");
        let props = acct
            .get("propertySummaries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if props.is_empty() {
            rows.push(vec![
                json!(account_id),
                json!(account_name),
                json!(""),
                json!("(no properties)"),
                json!(""),
            ]);
        }
        for p in props {
            let property = s(&p, "property");
            let property_id = property
                .strip_prefix("properties/")
                .unwrap_or(&property)
                .to_string();
            let kind = s(&p, "propertyType")
                .strip_prefix("PROPERTY_TYPE_")
                .map(str::to_lowercase)
                .unwrap_or_default();
            rows.push(vec![
                json!(account_id),
                json!(account_name),
                json!(property_id),
                json!(s(&p, "displayName")),
                json!(kind),
            ]);
        }
    }
    rows
}

async fn handle_properties(matches: &ArgMatches) -> Result<(), GwsError> {
    let format = table_by_default(matches);
    let url = format!("{ADMIN_BASE}/accountSummaries");
    if matches.get_flag("dry-run") {
        let out =
            json!({"dry_run": true, "method": "GET", "url": url, "query": {"pageSize": "200"}});
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return Ok(());
    }
    let token = auth::get_token(&[SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Analytics auth failed: {e}")))?;
    let client = crate::client::shared_client()?;

    let mut summaries: Vec<Value> = Vec::new();
    let mut page_token: Option<String> = None;
    // accountSummaries pages are 200 wide; 50 pages is far beyond any real account.
    for _ in 0..50 {
        let resp = crate::executor::fetch_json(
            || {
                let mut req = client
                    .get(&url)
                    .bearer_auth(&token)
                    .query(&[("pageSize", "200")]);
                if let Some(t) = &page_token {
                    req = req.query(&[("pageToken", t.as_str())]);
                }
                req
            },
            "Analytics accountSummaries",
        )
        .await?;
        if let Some(items) = resp.get("accountSummaries").and_then(Value::as_array) {
            summaries.extend(items.iter().cloned());
        }
        page_token = resp
            .get("nextPageToken")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        if page_token.is_none() {
            break;
        }
    }

    let columns: Vec<String> = [
        "account",
        "account_name",
        "property",
        "property_name",
        "type",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let rows = property_rows(&summaries);
    let rendered = crate::formatter::render_report(
        &columns,
        &rows,
        &json!({}),
        None,
        "No Google Analytics accounts or properties are visible to this login.",
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
        let root = Command::new("gws")
            .arg(Arg::new("format").long("format").global(true))
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .global(true)
                    .action(clap::ArgAction::SetTrue),
            );
        AnalyticsHelper.inject_commands(root, &Default::default())
    }

    fn opts(args: &[&str]) -> ReportOptions {
        let mut argv = vec!["gws", "+report"];
        argv.extend_from_slice(args);
        let m = cmd().try_get_matches_from(argv).unwrap();
        parse_options(m.subcommand_matches("+report").unwrap(), d("2026-11-23")).unwrap()
    }

    #[test]
    fn normalize_property_forms() {
        assert_eq!(
            normalize_property("562598057").unwrap(),
            "properties/562598057"
        );
        assert_eq!(
            normalize_property("properties/562598057").unwrap(),
            "properties/562598057"
        );
        assert!(normalize_property("G-ABC123").is_err());
        assert!(normalize_property("properties/../1").is_err());
        assert!(normalize_property("").is_err());
        assert!(normalize_property("12\u{0}3").is_err());
    }

    #[test]
    fn parse_metrics_validates() {
        assert_eq!(
            parse_metrics("activeUsers, sessions,activeUsers").unwrap(),
            vec!["activeUsers", "sessions"]
        );
        assert!(parse_metrics("customEvent:score").is_ok());
        assert!(parse_metrics("bad metric").is_err());
        assert!(parse_metrics("a/b").is_err());
        assert!(parse_metrics(",").is_err());
        let eleven = (0..11)
            .map(|i| format!("m{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_metrics(&eleven).is_err());
    }

    #[test]
    fn defaults() {
        let o = opts(&["562598057"]);
        assert_eq!(o.property, "properties/562598057");
        assert_eq!(o.dimensions, vec!["country"]);
        assert_eq!(o.metrics.len(), 5);
        assert_eq!(o.limit, 25);
        assert_eq!(o.start, d("2026-10-27"));
        assert_eq!(o.end, d("2026-11-23"));
    }

    #[test]
    fn by_accepts_listed_dimensions_case_insensitively() {
        for dim in [
            "country",
            "city",
            "deviceCategory",
            "sessionSource",
            "pagePath",
            "date",
            "userAgeBracket",
            "userGender",
        ] {
            assert_eq!(opts(&["1", "--by", dim]).dimensions, vec![dim]);
        }
        assert_eq!(
            opts(&["1", "--by", "devicecategory"]).dimensions,
            vec!["deviceCategory"]
        );
        let m = cmd()
            .try_get_matches_from(["gws", "+report", "1", "--by", "nope"])
            .unwrap();
        assert!(parse_options(m.subcommand_matches("+report").unwrap(), d("2026-11-23")).is_err());
    }

    #[test]
    fn report_body_orders_by_first_metric_or_date() {
        let o = opts(&["1", "--metrics", "sessions,activeUsers"]);
        let b = build_report_body(&o);
        assert_eq!(b["orderBys"][0]["metric"]["metricName"], "sessions");
        assert_eq!(b["orderBys"][0]["desc"], true);
        assert_eq!(b["dateRanges"][0]["startDate"], "2026-10-27");
        assert_eq!(b["dimensions"][0]["name"], "country");
        assert_eq!(b["metricAggregations"][0], "TOTAL");
        let o = opts(&["1", "--by", "date"]);
        let b = build_report_body(&o);
        assert_eq!(b["orderBys"][0]["dimension"]["dimensionName"], "date");
        assert_eq!(b["orderBys"][0]["desc"], false);
    }

    #[test]
    fn report_url_keeps_property_path() {
        assert_eq!(
            report_url("properties/562598057"),
            "https://analyticsdata.googleapis.com/v1beta/properties/562598057:runReport"
        );
    }

    fn sample() -> Value {
        json!({
            "dimensionHeaders": [{"name": "date"}],
            "metricHeaders": [
                {"name": "activeUsers", "type": "TYPE_INTEGER"},
                {"name": "engagementRate", "type": "TYPE_FLOAT"},
                {"name": "averageSessionDuration", "type": "TYPE_SECONDS"}
            ],
            "rows": [
                {"dimensionValues": [{"value": "20261122"}],
                 "metricValues": [{"value": "42"}, {"value": "0.61234"}, {"value": "83.456"}]}
            ],
            "totals": [
                {"dimensionValues": [{"value": "RESERVED_TOTAL"}],
                 "metricValues": [{"value": "42"}, {"value": "0.61234"}, {"value": "83.456"}]}
            ],
            "rowCount": 1
        })
    }

    #[test]
    fn build_rows_formats_by_type() {
        let o = opts(&[
            "1",
            "--by",
            "date",
            "--metrics",
            "activeUsers,engagementRate,averageSessionDuration",
        ]);
        let (cols, rows) = build_rows(&sample(), &o, true);
        assert_eq!(
            cols,
            vec![
                "date",
                "activeUsers",
                "engagementRate",
                "averageSessionDuration"
            ]
        );
        assert_eq!(rows[0][0], "2026-11-22");
        assert_eq!(rows[0][1], 42);
        assert_eq!(rows[0][2], "61.2%");
        assert_eq!(rows[0][3], "83.5s");
        let (_, rows) = build_rows(&sample(), &o, false);
        assert_eq!(rows[0][2], 0.6123);
        assert_eq!(rows[0][3], 83.456);
    }

    #[test]
    fn totals_and_title() {
        let o = opts(&[
            "1",
            "--by",
            "date",
            "--metrics",
            "activeUsers,engagementRate,averageSessionDuration",
        ]);
        let t = totals(&sample(), &o).unwrap();
        assert_eq!(t["activeUsers"], 42);
        let title = title_line(&o, Some(&t), 1);
        assert!(title.contains("properties/1 · 2026-10-27 → 2026-11-23 (28 days) · by date"));
        assert!(title.contains("activeUsers 42"));
        assert!(title.contains("engagementRate 61.2%"));
        assert!(totals(&json!({}), &o).is_none());
    }

    #[test]
    fn empty_response_gives_no_rows_and_clear_note() {
        let o = opts(&["562598057"]);
        let (_, rows) = build_rows(&json!({"metricHeaders": []}), &o, true);
        assert!(rows.is_empty());
        let note = empty_note(&o);
        assert!(note.starts_with("No data in range"));
        assert!(note.contains("GA4"));
    }

    #[test]
    fn pretty_date_only_touches_yyyymmdd() {
        assert_eq!(pretty_date("20261123"), "2026-11-23");
        assert_eq!(pretty_date("(other)"), "(other)");
    }

    #[test]
    fn metric_cell_non_numeric_passthrough() {
        assert_eq!(metric_cell("n/a", "TYPE_INTEGER", "x", true), json!("n/a"));
        assert_eq!(metric_cell("1.5", "TYPE_FLOAT", "x", true), json!("1.50"));
    }

    #[test]
    fn scope_errors_get_a_hint() {
        let e = explain_scope_error(GwsError::Api {
            code: 403,
            message: "Request had insufficient authentication scopes.".into(),
            reason: "unknown".into(),
            enable_url: None,
        });
        let msg = e.to_string();
        assert!(msg.contains("analytics.readonly"), "{msg}");
        assert!(msg.contains("gws auth login --scopes"), "{msg}");
        let other = explain_scope_error(GwsError::Validation("x".into()));
        assert_eq!(other.to_string(), "x");
    }

    #[test]
    fn property_rows_flatten_accounts() {
        let rows = property_rows(&[
            json!({
                "account": "accounts/414878903",
                "displayName": "Ads",
                "propertySummaries": [{
                    "property": "properties/562598057",
                    "displayName": "www.mbprep.app",
                    "propertyType": "PROPERTY_TYPE_ORDINARY"
                }]
            }),
            json!({"account": "accounts/2", "displayName": "Empty"}),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "414878903");
        assert_eq!(rows[0][2], "562598057");
        assert_eq!(rows[0][3], "www.mbprep.app");
        assert_eq!(rows[0][4], "ordinary");
        assert_eq!(rows[1][3], "(no properties)");
    }
}

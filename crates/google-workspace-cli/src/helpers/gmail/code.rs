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

//! Gmail `+code` helper — prints the one-time code from the newest matching
//! email, and nothing else.
//!
//! Designed for `code=$(gws gmail +code 'from:no-reply@example.com')`: stdout
//! carries only the code, and every failure (no code, auth, API) goes to
//! stderr with a non-zero exit status, so a failed lookup can never be
//! mistaken for a code.

use super::*;
use regex::{Regex, RegexBuilder};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default code shape: 4–8 digits, or two groups of 3–4 digits split by one
/// space ("123 456"). The split form is tried first so it wins over its halves.
const DEFAULT_CODE_PATTERN: &str = r"\b(\d{3,4} \d{3,4}|\d{4,8})\b";

/// Words that, shortly before a candidate, mark it as the code.
const CODE_KEYWORDS: &[&str] = &[
    "code",
    "otp",
    "pin",
    "passcode",
    "password",
    "verification",
    "verify",
    "one-time",
    "one time",
    "token",
    "security",
];

/// How far before a candidate (in characters) to look for a keyword.
const KEYWORD_WINDOW: usize = 60;

/// Seconds between polls when `--wait` is set.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

const MAX_WITHIN: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_WAIT: Duration = Duration::from_secs(30 * 60);
const MAX_PATTERN_LEN: usize = 512;
const NO_CODE_EXIT_CODE: i32 = 1;

/// Validated `+code` arguments.
#[derive(Debug)]
struct CodeOptions {
    query: String,
    within: Duration,
    wait: Duration,
    pattern: Option<Regex>,
}

/// Why `+code` printed nothing.
#[derive(Debug)]
enum CodeError {
    /// The lookup worked but produced no code; the message says why.
    NotFound(String),
    Gws(GwsError),
}

impl From<GwsError> for CodeError {
    fn from(e: GwsError) -> Self {
        CodeError::Gws(e)
    }
}

/// Handle the `+code` subcommand.
///
/// Errors are reported on stderr and terminate the process here rather than
/// propagating to `main`, whose error path prints JSON to stdout: a caller
/// capturing stdout must receive the code or nothing at all.
pub(super) async fn handle_code(matches: &ArgMatches) -> Result<(), GwsError> {
    let result = match parse_code_options(matches) {
        Ok(opts) => run_code(&opts).await,
        Err(e) => Err(CodeError::Gws(e)),
    };
    match result {
        Ok(code) => {
            println!("{code}");
            Ok(())
        }
        Err(err) => {
            let (message, exit_code) = describe_error(&err);
            eprintln!("{message}");
            std::process::exit(exit_code);
        }
    }
}

/// Stderr message and exit status for a failed lookup.
fn describe_error(err: &CodeError) -> (String, i32) {
    match err {
        CodeError::NotFound(msg) => (
            format!("error: {}", sanitize_for_terminal(msg)),
            NO_CODE_EXIT_CODE,
        ),
        CodeError::Gws(e) => (
            format!("error: {}", sanitize_for_terminal(&e.to_string())),
            e.exit_code(),
        ),
    }
}

fn parse_code_options(matches: &ArgMatches) -> Result<CodeOptions, GwsError> {
    let query = matches
        .get_one::<String>("query")
        .ok_or_else(|| GwsError::Validation("A search query is required".to_string()))?;
    let query = validate_search_query(query)?.to_string();

    let within = parse_duration(
        matches.get_one::<String>("within").map_or("10m", |s| s),
        "--within",
    )?;
    if within.is_zero() || within > MAX_WITHIN {
        return Err(GwsError::Validation(
            "--within must be between 1s and 30d".to_string(),
        ));
    }

    let wait = parse_duration(
        matches.get_one::<String>("wait").map_or("0s", |s| s),
        "--wait",
    )?;
    if wait > MAX_WAIT {
        return Err(GwsError::Validation(
            "--wait must be at most 30m".to_string(),
        ));
    }

    let pattern = matches
        .get_one::<String>("pattern")
        .map(|p| compile_pattern(p))
        .transpose()?;

    Ok(CodeOptions {
        query,
        within,
        wait,
        pattern,
    })
}

/// Parse `90`, `90s`, `10m`, `2h` or `1d` into a duration (bare numbers are seconds).
fn parse_duration(raw: &str, flag: &str) -> Result<Duration, GwsError> {
    let s = raw.trim();
    let invalid = || {
        GwsError::Validation(format!(
            "{flag} must be a number with an optional unit s, m, h or d (e.g. 90s, 10m); got '{}'",
            sanitize_for_terminal(raw)
        ))
    };
    let (digits, unit) = match s.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((idx, _)) => s.split_at(idx),
        None => (s, "s"),
    };
    if digits.is_empty() {
        return Err(invalid());
    }
    let value: u64 = digits.parse().map_err(|_| invalid())?;
    let multiplier: u64 = match unit.to_ascii_lowercase().as_str() {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return Err(invalid()),
    };
    value
        .checked_mul(multiplier)
        .map(Duration::from_secs)
        .ok_or_else(invalid)
}

/// Compile a user-supplied `--pattern`, bounding its size.
fn compile_pattern(raw: &str) -> Result<Regex, GwsError> {
    if raw.is_empty() {
        return Err(GwsError::Validation(
            "--pattern must not be empty".to_string(),
        ));
    }
    if raw.len() > MAX_PATTERN_LEN {
        return Err(GwsError::Validation(format!(
            "--pattern must be at most {MAX_PATTERN_LEN} characters"
        )));
    }
    crate::validate::reject_dangerous_chars(raw, "--pattern")?;
    RegexBuilder::new(raw)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| {
            GwsError::Validation(format!(
                "--pattern is not a valid regex: {}",
                sanitize_for_terminal(&e.to_string())
            ))
        })
}

/// Restrict the user's query to mail received after `cutoff_secs` (Unix time).
fn windowed_query(query: &str, cutoff_secs: u64) -> String {
    format!("({query}) after:{cutoff_secs}")
}

/// Search, fetch and extract, polling until `opts.wait` runs out.
async fn run_code(opts: &CodeOptions) -> Result<String, CodeError> {
    let token = auth::get_token(&[GMAIL_READONLY_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Gmail auth failed: {e}")))?;
    let client = crate::client::build_client()?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| GwsError::Other(anyhow::anyhow!("System clock error: {e}")))?
        .as_millis() as u64;
    let cutoff_ms = now_ms.saturating_sub(opts.within.as_millis() as u64);
    let query = windowed_query(&opts.query, cutoff_ms / 1000);

    let fetch_newest = || {
        let client = &client;
        let token = token.as_str();
        let query = query.as_str();
        async move {
            let ids = search_message_ids(client, token, query, 1).await?;
            match ids.first() {
                Some(id) => Ok(Some(fetch_message_json(client, token, id).await?)),
                None => Ok(None),
            }
        }
    };

    poll_for_code(fetch_newest, opts, cutoff_ms, POLL_INTERVAL).await
}

/// Repeatedly fetch the newest matching message until a code is found or the
/// wait budget is spent. `fetch` returns the raw Gmail message JSON, or `None`
/// when nothing matches.
async fn poll_for_code<F, Fut>(
    mut fetch: F,
    opts: &CodeOptions,
    cutoff_ms: u64,
    interval: Duration,
) -> Result<String, CodeError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<Value>, GwsError>>,
{
    let deadline = Instant::now() + opts.wait;
    loop {
        let outcome = match fetch().await? {
            Some(msg) => code_from_message(&msg, cutoff_ms, opts.pattern.as_ref()),
            None => Err(format!(
                "no message matching '{}' in the last {}",
                opts.query,
                format_duration(opts.within)
            )),
        };
        let reason = match outcome {
            Ok(code) => return Ok(code),
            Err(reason) => reason,
        };

        let now = Instant::now();
        if now >= deadline {
            let waited = if opts.wait.is_zero() {
                String::new()
            } else {
                format!(" after waiting {}", format_duration(opts.wait))
            };
            return Err(CodeError::NotFound(format!("{reason}{waited}")));
        }
        tokio::time::sleep(interval.min(deadline - now)).await;
    }
}

/// Extract the code from one message, or explain why there is none.
fn code_from_message(
    msg: &Value,
    cutoff_ms: u64,
    pattern: Option<&Regex>,
) -> Result<String, String> {
    let received_ms = msg
        .get("internalDate")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());
    if received_ms.is_some_and(|r| r < cutoff_ms) {
        return Err("the newest matching message is older than the --within window".to_string());
    }

    let (subject, body) = message_texts(msg);
    find_code(&subject, pattern)
        .or_else(|| find_code(&body, pattern))
        .ok_or_else(|| {
            format!(
                "no code found in the newest matching message (subject: '{}')",
                subject.replace(['\r', '\n'], " ")
            )
        })
}

/// Subject and readable body of a raw Gmail message. The body prefers
/// text/plain, then HTML converted to text, then the snippet.
fn message_texts(msg: &Value) -> (String, String) {
    let payload = msg.get("payload");
    let subject = payload
        .and_then(|p| p.get("headers"))
        .and_then(|h| h.as_array())
        .map(|headers| parse_message_headers(headers).subject)
        .unwrap_or_default();

    let contents = payload.map(extract_payload_contents).unwrap_or_default();
    let body = contents
        .body_text
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            contents
                .body_html
                .as_deref()
                .map(html_text::html_to_text)
                .filter(|t| !t.trim().is_empty())
        })
        .or_else(|| {
            msg.get("snippet")
                .and_then(|s| s.as_str())
                .map(html_text::decode_entities)
        })
        .unwrap_or_default();
    (subject, body)
}

/// Find the code in `text`. With a custom pattern, the first match wins
/// (capture group 1 when the pattern has one). With the default pattern,
/// a candidate shortly after a word like "code" or "OTP" wins; otherwise the
/// first candidate that is not a year. Whitespace is removed from the result.
fn find_code(text: &str, pattern: Option<&Regex>) -> Option<String> {
    let raw = match pattern {
        Some(re) => re.captures_iter(text).find_map(|caps| {
            caps.get(1)
                .or_else(|| caps.get(0))
                .map(|m| m.as_str().to_string())
        }),
        None => find_default_code(text),
    }?;
    let code: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    (!code.is_empty()).then_some(code)
}

fn find_default_code(text: &str) -> Option<String> {
    static DEFAULT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static URL: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = DEFAULT.get_or_init(|| Regex::new(DEFAULT_CODE_PATTERN).expect("valid default"));
    let url = URL.get_or_init(|| Regex::new(r"https?://\S+").expect("valid url regex"));

    // Digits inside links (tracking IDs, tokens) are not the code.
    let text = url.replace_all(text, " ");
    // ASCII lowercasing keeps byte offsets identical to `text`.
    let lower = text.to_ascii_lowercase();

    let candidates: Vec<regex::Match<'_>> = re.find_iter(&text).collect();
    let near_keyword = candidates.iter().find(|m| {
        let start = floor_char_boundary(&lower, m.start().saturating_sub(KEYWORD_WINDOW));
        let before = &lower[start..m.start()];
        CODE_KEYWORDS.iter().any(|k| before.contains(k))
    });
    near_keyword
        .or_else(|| candidates.iter().find(|m| !looks_like_year(m.as_str())))
        .map(|m| m.as_str().to_string())
}

/// A four-digit number between 1900 and 2099 is more likely a year than a code.
fn looks_like_year(candidate: &str) -> bool {
    candidate.len() == 4
        && candidate
            .parse::<u32>()
            .is_ok_and(|y| (1900..=2099).contains(&y))
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        s if s % 86_400 == 0 && s > 0 => format!("{}d", s / 86_400),
        s if s % 3_600 == 0 && s > 0 => format!("{}h", s / 3_600),
        s if s % 60 == 0 && s > 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

use crate::output::sanitize_for_terminal;

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(subject: &str, plain: Option<&str>, html: Option<&str>, internal_ms: u64) -> Value {
        let mut parts = Vec::new();
        if let Some(p) = plain {
            parts.push(json!({"mimeType": "text/plain", "body": {"data": URL_SAFE.encode(p)}}));
        }
        if let Some(h) = html {
            parts.push(json!({"mimeType": "text/html", "body": {"data": URL_SAFE.encode(h)}}));
        }
        json!({
            "id": "m1",
            "internalDate": internal_ms.to_string(),
            "snippet": "snippet text",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    {"name": "From", "value": "hello@example.com"},
                    {"name": "Subject", "value": subject}
                ],
                "parts": parts
            }
        })
    }

    fn opts(wait: Duration, pattern: Option<&str>) -> CodeOptions {
        CodeOptions {
            query: "from:hello@example.com".to_string(),
            within: Duration::from_secs(600),
            wait,
            pattern: pattern.map(|p| compile_pattern(p).unwrap()),
        }
    }

    // --- find_code ---

    #[test]
    fn default_finds_plain_digits() {
        assert_eq!(
            find_code("Your code is 482913.", None).as_deref(),
            Some("482913")
        );
        assert_eq!(find_code("PIN: 4829", None).as_deref(), Some("4829"));
        assert_eq!(find_code("12345678", None).as_deref(), Some("12345678"));
    }

    #[test]
    fn default_joins_space_split_code() {
        assert_eq!(find_code("Code: 123 456", None).as_deref(), Some("123456"));
        assert_eq!(
            find_code("Use 1234 5678 now", None).as_deref(),
            Some("12345678")
        );
    }

    #[test]
    fn default_rejects_too_short_or_long() {
        assert_eq!(find_code("only 123 here", None), None);
        assert_eq!(find_code("123456789", None), None);
    }

    #[test]
    fn default_prefers_candidate_after_keyword() {
        let text = "Order 55512345 shipped. Your verification code is 908172.";
        assert_eq!(find_code(text, None).as_deref(), Some("908172"));
    }

    #[test]
    fn default_skips_years_without_keyword() {
        assert_eq!(find_code("Welcome to 2026!", None), None);
        assert_eq!(
            find_code("(c) 2026 Example. Ref 7731", None).as_deref(),
            Some("7731")
        );
        // A year-shaped code right after a keyword is still a code.
        assert_eq!(find_code("Your code: 2026", None).as_deref(), Some("2026"));
    }

    #[test]
    fn default_ignores_digits_in_links() {
        let text = "Sign in at https://example.com/l?id=99887766 or enter code 445566";
        assert_eq!(find_code(text, None).as_deref(), Some("445566"));
        assert_eq!(find_code("https://e.com/12345678", None), None);
    }

    #[test]
    fn default_handles_multibyte_text_before_candidate() {
        let text = "Ваш код подтверждения — это число: ééééééééééééééééééééééééééééééé 112233";
        assert_eq!(find_code(text, None).as_deref(), Some("112233"));
    }

    #[test]
    fn custom_pattern_uses_group_or_whole_match() {
        let re = compile_pattern(r"[A-Z0-9]{6}").unwrap();
        assert_eq!(
            find_code("token AB12CD here", Some(&re)).as_deref(),
            Some("AB12CD")
        );
        let re = compile_pattern(r"code ([a-z]{3}-[a-z]{3})").unwrap();
        assert_eq!(
            find_code("your code abc-def", Some(&re)).as_deref(),
            Some("abc-def")
        );
        let re = compile_pattern(r"X(\d)? Y").unwrap();
        assert_eq!(find_code("X Y", Some(&re)).as_deref(), Some("XY"));
        let re = compile_pattern(r"\s+").unwrap();
        assert_eq!(find_code("a b", Some(&re)), None);
    }

    // --- argument parsing ---

    #[test]
    fn parse_duration_accepts_units() {
        assert_eq!(
            parse_duration("90", "--x").unwrap(),
            Duration::from_secs(90)
        );
        assert_eq!(
            parse_duration("90s", "--x").unwrap(),
            Duration::from_secs(90)
        );
        assert_eq!(
            parse_duration("10m", "--x").unwrap(),
            Duration::from_secs(600)
        );
        assert_eq!(
            parse_duration(" 2H ", "--x").unwrap(),
            Duration::from_secs(7200)
        );
        assert_eq!(
            parse_duration("1d", "--x").unwrap(),
            Duration::from_secs(86_400)
        );
        assert_eq!(parse_duration("0s", "--x").unwrap(), Duration::ZERO);
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        for bad in [
            "",
            "m",
            "10x",
            "1.5m",
            "-5s",
            "10mm",
            "99999999999999999999d",
            "18446744073709551615d",
        ] {
            assert!(
                parse_duration(bad, "--within").is_err(),
                "{bad} should fail"
            );
        }
    }

    #[test]
    fn compile_pattern_validates() {
        assert!(compile_pattern("").is_err());
        assert!(compile_pattern("(unclosed").is_err());
        assert!(compile_pattern(&"a".repeat(MAX_PATTERN_LEN + 1)).is_err());
        assert!(compile_pattern("a\u{7}").is_err());
        assert!(compile_pattern(r"\d{6}").is_ok());
    }

    fn parse_code(args: &[&str]) -> Result<CodeOptions, String> {
        let cmd = GmailHelper.inject_commands(
            Command::new("gmail"),
            &crate::discovery::RestDescription::default(),
        );
        let mut argv = vec!["gmail", "+code"];
        argv.extend_from_slice(args);
        let m = cmd.try_get_matches_from(argv).map_err(|e| e.to_string())?;
        parse_code_options(m.subcommand_matches("+code").unwrap()).map_err(|e| e.to_string())
    }

    #[test]
    fn code_options_defaults_and_bounds() {
        let o = parse_code(&["from:a@example.com"]).unwrap();
        assert_eq!(o.query, "from:a@example.com");
        assert_eq!(o.within, Duration::from_secs(600));
        assert_eq!(o.wait, Duration::ZERO);
        assert!(o.pattern.is_none());

        let o = parse_code(&["q", "--within", "1h", "--wait", "60s", "--pattern", r"\d+"]).unwrap();
        assert_eq!(o.within, Duration::from_secs(3600));
        assert_eq!(o.wait, Duration::from_secs(60));
        assert!(o.pattern.is_some());

        assert!(parse_code(&[]).is_err());
        assert!(parse_code(&["  "]).is_err());
        assert!(parse_code(&["q", "--within", "0s"]).is_err());
        assert!(parse_code(&["q", "--within", "31d"]).is_err());
        assert!(parse_code(&["q", "--wait", "31m"]).is_err());
        assert!(parse_code(&["q", "--pattern", "("]).is_err());
    }

    #[test]
    fn windowed_query_groups_user_query() {
        assert_eq!(
            windowed_query("from:a OR from:b", 1_700_000_000),
            "(from:a OR from:b) after:1700000000"
        );
    }

    // --- message handling ---

    #[test]
    fn subject_is_checked_before_body() {
        let m = msg("123456 is your code", Some("Use 654321"), None, 2_000);
        assert_eq!(code_from_message(&m, 1_000, None).unwrap(), "123456");
    }

    #[test]
    fn body_plain_then_html() {
        let m = msg(
            "Sign in",
            Some("Your code: 111 222"),
            Some("<p>333444</p>"),
            2_000,
        );
        assert_eq!(code_from_message(&m, 1_000, None).unwrap(), "111222");
        let m = msg(
            "Sign in",
            None,
            Some("<style>.c{color:#123456}</style><p>Your code</p><td>778899</td>"),
            2_000,
        );
        assert_eq!(code_from_message(&m, 1_000, None).unwrap(), "778899");
    }

    #[test]
    fn snippet_is_last_resort() {
        let mut m = msg("Sign in", None, None, 2_000);
        m["snippet"] = json!("Your code is 5566&#39;s");
        assert_eq!(code_from_message(&m, 1_000, None).unwrap(), "5566");
    }

    #[test]
    fn message_older_than_window_is_rejected() {
        let m = msg("123456", None, None, 500);
        let err = code_from_message(&m, 1_000, None).unwrap_err();
        assert!(err.contains("older than"));
    }

    #[test]
    fn missing_internal_date_is_accepted() {
        let mut m = msg("Code 123456", None, None, 0);
        m.as_object_mut().unwrap().remove("internalDate");
        assert_eq!(code_from_message(&m, 1_000, None).unwrap(), "123456");
    }

    #[test]
    fn no_code_message_names_subject() {
        let m = msg("Welcome\r\naboard", Some("Thanks for joining"), None, 2_000);
        let err = code_from_message(&m, 1_000, None).unwrap_err();
        assert!(err.contains("Welcome  aboard"));
    }

    #[test]
    fn message_texts_handles_missing_payload() {
        let (s, b) = message_texts(&json!({}));
        assert!(s.is_empty());
        assert!(b.is_empty());
    }

    // --- polling ---

    #[tokio::test]
    async fn poll_returns_first_code() {
        let o = opts(Duration::ZERO, None);
        let code = poll_for_code(
            || async { Ok(Some(msg("Code 246810", None, None, 2_000))) },
            &o,
            1_000,
            Duration::from_millis(1),
        )
        .await
        .unwrap();
        assert_eq!(code, "246810");
    }

    #[tokio::test]
    async fn poll_without_wait_reports_no_message() {
        let o = opts(Duration::ZERO, None);
        let err = poll_for_code(|| async { Ok(None) }, &o, 1_000, Duration::from_millis(1))
            .await
            .unwrap_err();
        match err {
            CodeError::NotFound(m) => {
                assert!(m.contains("no message matching 'from:hello@example.com'"));
                assert!(m.contains("10m"));
                assert!(!m.contains("after waiting"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn poll_waits_until_code_arrives() {
        let o = opts(Duration::from_secs(5), None);
        let calls = std::cell::Cell::new(0);
        let code = poll_for_code(
            || {
                calls.set(calls.get() + 1);
                let n = calls.get();
                async move {
                    Ok(match n {
                        1 => None,
                        2 => Some(msg("Welcome", Some("no digits"), None, 2_000)),
                        _ => Some(msg("Your code", Some("code 135790"), None, 2_000)),
                    })
                }
            },
            &o,
            1_000,
            Duration::from_millis(1),
        )
        .await
        .unwrap();
        assert_eq!(code, "135790");
        assert_eq!(calls.get(), 3);
    }

    #[tokio::test]
    async fn poll_gives_up_after_wait() {
        let o = opts(Duration::from_millis(20), None);
        let err = poll_for_code(
            || async { Ok(Some(msg("Welcome", Some("hello"), None, 2_000))) },
            &o,
            1_000,
            Duration::from_millis(5),
        )
        .await
        .unwrap_err();
        match err {
            CodeError::NotFound(m) => {
                assert!(m.contains("no code found"));
                assert!(m.contains("after waiting 0s"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn poll_propagates_api_errors() {
        let o = opts(Duration::from_secs(5), None);
        let err = poll_for_code(
            || async { Err(GwsError::Auth("expired".to_string())) },
            &o,
            1_000,
            Duration::from_millis(1),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CodeError::Gws(GwsError::Auth(_))));
    }

    #[test]
    fn describe_error_uses_stderr_text_and_exit_codes() {
        let (m, c) = describe_error(&CodeError::NotFound("nothing\x1b[2J".to_string()));
        assert_eq!(m, "error: nothing[2J");
        assert_eq!(c, 1);
        let (m, c) = describe_error(&CodeError::Gws(GwsError::Validation("bad".to_string())));
        assert_eq!(m, "error: bad");
        assert_eq!(c, GwsError::EXIT_CODE_VALIDATION);
        let e: CodeError = GwsError::Auth("x".to_string()).into();
        assert_eq!(describe_error(&e).1, GwsError::EXIT_CODE_AUTH);
    }

    #[test]
    fn format_duration_picks_largest_unit() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0s");
        assert_eq!(format_duration(Duration::from_secs(90)), "90s");
        assert_eq!(format_duration(Duration::from_secs(600)), "10m");
        assert_eq!(format_duration(Duration::from_secs(7200)), "2h");
        assert_eq!(format_duration(Duration::from_secs(86_400)), "1d");
    }

    #[test]
    fn looks_like_year_bounds() {
        assert!(looks_like_year("1999"));
        assert!(looks_like_year("2026"));
        assert!(!looks_like_year("2100"));
        assert!(!looks_like_year("1899"));
        assert!(!looks_like_year("20260"));
    }
}

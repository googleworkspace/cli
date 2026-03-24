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

//! Account timezone resolution for Google Workspace CLI.
//!
//! Resolves the authenticated user's timezone with the following priority:
//! 1. Explicit `--timezone` CLI flag (hard error if invalid)
//! 2. Cached value from config dir (24h TTL)
//! 3. Google Calendar Settings API (`users/me/settings/timezone`)
//! 4. Machine-local timezone (fallback with warning)

use crate::error::GwsError;
use chrono_tz::Tz;
use std::path::PathBuf;

/// Cache filename stored in the gws config directory.
const CACHE_FILENAME: &str = "account_timezone";

/// Cache TTL in seconds (24 hours).
const CACHE_TTL_SECS: u64 = 86400;

/// Returns the path to the timezone cache file.
fn cache_path() -> PathBuf {
    crate::auth_commands::config_dir().join(CACHE_FILENAME)
}

/// Remove the cached timezone file. Called on auth login/logout to
/// invalidate stale values when the account changes.
pub fn invalidate_cache() {
    let path = cache_path();
    if let Err(e) = std::fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), error = %e, "failed to invalidate timezone cache");
        }
    }
}

/// Read the cached timezone if it exists and is fresh (< 24h old).
fn read_cache() -> Option<Tz> {
    let path = cache_path();
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = std::time::SystemTime::now().duration_since(modified).ok()?;
    if age.as_secs() > CACHE_TTL_SECS {
        return None;
    }
    let contents = std::fs::read_to_string(&path).ok()?;
    let tz_name = contents.trim();
    tz_name.parse::<Tz>().ok()
}

/// Write a timezone name to the cache file.
fn write_cache(tz_name: &str) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(path = %parent.display(), error = %e, "failed to create timezone cache directory");
            return;
        }
    }
    if let Err(e) = std::fs::write(&path, tz_name) {
        tracing::warn!(path = %path.display(), error = %e, "failed to write timezone cache");
    }
}

/// Fetch the account timezone from the Google Calendar Settings API.
async fn fetch_account_timezone(client: &reqwest::Client, token: &str) -> Result<Tz, GwsError> {
    let url = "https://www.googleapis.com/calendar/v3/users/me/settings/timezone";
    let resp = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to fetch account timezone: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(GwsError::Api {
            code: status.as_u16(),
            message: body,
            reason: "timezone_fetch_failed".to_string(),
            enable_url: None,
        });
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| GwsError::Other(anyhow::anyhow!("Failed to parse timezone response: {e}")))?;

    let tz_name = json
        .get("value")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            GwsError::Other(anyhow::anyhow!(
                "Timezone setting missing or empty 'value' field"
            ))
        })?;

    let tz: Tz = tz_name.parse().map_err(|_| {
        GwsError::Other(anyhow::anyhow!(
            "Google returned unrecognized timezone: {tz_name}"
        ))
    })?;

    // Cache for future use
    write_cache(tz_name);
    tracing::info!(
        timezone = tz_name,
        source = "calendar_api",
        "resolved account timezone"
    );

    Ok(tz)
}

/// Parse an explicit timezone string, returning an error if invalid.
pub fn parse_timezone(tz_str: &str) -> Result<Tz, GwsError> {
    tz_str.parse::<Tz>().map_err(|_| {
        GwsError::Validation(format!(
            "Invalid timezone '{tz_str}'. Use an IANA timezone name (e.g. America/Denver, Europe/London, UTC)."
        ))
    })
}

/// Resolve the user's timezone with this priority:
/// 1. `tz_override` (from `--timezone` flag) — hard error if invalid
/// 2. Cached value in config dir — use if < 24h old
/// 3. Google Calendar Settings API — fetch and cache
/// 4. Machine-local timezone (log warning)
pub async fn resolve_account_timezone(
    client: &reqwest::Client,
    token: &str,
    tz_override: Option<&str>,
) -> Result<Tz, GwsError> {
    // 1. Explicit override — fail if invalid
    if let Some(tz_str) = tz_override {
        let tz = parse_timezone(tz_str)?;
        tracing::info!(
            timezone = tz_str,
            source = "cli_flag",
            "using explicit timezone"
        );
        return Ok(tz);
    }

    // 2. Check cache
    if let Some(tz) = read_cache() {
        tracing::debug!(timezone = %tz, source = "cache", "using cached timezone");
        return Ok(tz);
    }

    // 3. Fetch from Calendar Settings API
    match fetch_account_timezone(client, token).await {
        Ok(tz) => return Ok(tz),
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch account timezone, falling back to local");
        }
    }

    // 4. Fall back to machine-local timezone
    let local_iana = iana_time_zone_fallback();
    tracing::warn!(
        timezone = local_iana.as_str(),
        source = "local_machine",
        "using machine-local timezone as fallback"
    );
    let tz: Tz = local_iana.parse().unwrap_or(chrono_tz::UTC);
    Ok(tz)
}

/// Return the start of today (midnight) in the given timezone as a
/// timezone-aware `DateTime`. Errors if midnight cannot be resolved
/// (e.g. a DST transition that skips midnight — extremely rare).
pub fn start_of_today(tz: Tz) -> Result<chrono::DateTime<Tz>, crate::error::GwsError> {
    use chrono::{NaiveTime, TimeZone, Utc};

    let now_in_tz = Utc::now().with_timezone(&tz);
    let today_start = now_in_tz
        .date_naive()
        .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    tz.from_local_datetime(&today_start)
        .earliest()
        .ok_or_else(|| {
            crate::error::GwsError::Other(anyhow::anyhow!(
                "Could not determine start of day in timezone '{}'",
                tz
            ))
        })
}

/// Best-effort machine-local IANA timezone detection using the
/// `iana-time-zone` crate, which reads the OS timezone database.
fn iana_time_zone_fallback() -> String {
    match iana_time_zone::get_timezone() {
        Ok(tz) => tz,
        Err(_) => "UTC".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_iana_timezone() {
        let tz = parse_timezone("America/Denver").unwrap();
        assert_eq!(tz, chrono_tz::America::Denver);
    }

    #[test]
    fn parse_utc_timezone() {
        let tz = parse_timezone("UTC").unwrap();
        assert_eq!(tz, chrono_tz::UTC);
    }

    #[test]
    fn parse_invalid_timezone_fails() {
        let result = parse_timezone("Not/A/Zone");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid timezone"));
        assert!(err.contains("Not/A/Zone"));
    }

    #[test]
    fn parse_empty_string_fails() {
        let result = parse_timezone("");
        assert!(result.is_err());
    }

    #[test]
    fn cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_file = dir.path().join(CACHE_FILENAME);

        // Write directly to test location
        std::fs::write(&cache_file, "America/New_York").unwrap();
        let contents = std::fs::read_to_string(&cache_file).unwrap();
        let tz: Tz = contents.trim().parse().unwrap();
        assert_eq!(tz, chrono_tz::America::New_York);
    }

    #[test]
    fn iana_fallback_returns_valid_tz() {
        let tz_name = iana_time_zone_fallback();
        // Should be parseable
        let result: Result<Tz, _> = tz_name.parse();
        assert!(
            result.is_ok(),
            "Fallback timezone '{tz_name}' should be parseable"
        );
    }
}

use super::*;

#[derive(Debug, PartialEq)]
pub struct RenewConfig {
    pub name: Option<String>,
    pub all: bool,
    pub within: String,
}

fn parse_renew_args(matches: &ArgMatches) -> Result<RenewConfig, GwsError> {
    let name = matches.get_one::<String>("name").cloned();
    let all = matches.get_flag("all");
    let within = matches
        .get_one::<String>("within")
        .cloned()
        .unwrap_or_else(|| "1h".to_string());

    if name.is_none() && !all {
        return Err(GwsError::Validation(
            "Either --name or --all is required for +renew".to_string(),
        ));
    }

    Ok(RenewConfig { name, all, within })
}

/// Handles the `+renew` command.
pub(super) async fn handle_renew(
    _doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_renew_args(matches)?;
    let client = crate::client::build_client()?;
    let ws_token = auth::get_token(&[WORKSPACE_EVENTS_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Failed to get token: {e}")))?;

    if let Some(name) = config.name {
        // Reactivate a specific subscription
        let name = crate::validate::validate_resource_name(&name)?;
        eprintln!("Reactivating subscription: {name}");
        let resp = client
            .post(format!(
                "https://workspaceevents.googleapis.com/v1/{name}:reactivate"
            ))
            .bearer_auth(&ws_token)
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await
            .context("Failed to reactivate subscription")?;

        let body: Value = resp.json().await.context("Failed to parse response")?;

        println!(
            "{}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );
    } else {
        let within_secs = parse_duration(&config.within)?;

        // List all subscriptions
        let resp = client
            .get("https://workspaceevents.googleapis.com/v1/subscriptions")
            .bearer_auth(&ws_token)
            .send()
            .await
            .context("Failed to list subscriptions")?;

        let body: Value = resp.json().await.context("Failed to parse response")?;

        let mut renewed = 0;
        if let Some(subs) = body.get("subscriptions").and_then(|s| s.as_array()) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let to_renew = filter_subscriptions_to_renew(subs, now, within_secs);

            for name in to_renew {
                let name = crate::validate::validate_resource_name(&name)?;
                eprintln!("Renewing {name}...");
                let _ = client
                    .post(format!(
                        "https://workspaceevents.googleapis.com/v1/{name}:reactivate"
                    ))
                    .bearer_auth(&ws_token)
                    .header("Content-Type", "application/json")
                    .body("{}")
                    .send()
                    .await;
                renewed += 1;
            }
        }

        let result = json!({
            "status": "success",
            "renewed": renewed,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
    }

    Ok(())
}

fn filter_subscriptions_to_renew(subs: &[Value], now_secs: u64, within_secs: u64) -> Vec<String> {
    let mut result = Vec::new();
    for sub in subs {
        if let Some(expire_time) = sub.get("expireTime").and_then(|e| e.as_str()) {
            if let Some(expire_secs) = parse_rfc3339_rough(expire_time) {
                let remaining = expire_secs.saturating_sub(now_secs);
                if remaining < within_secs {
                    if let Some(name) = sub.get("name").and_then(|n| n.as_str()) {
                        result.push(name.to_string());
                    }
                }
            }
        }
    }
    result
}

/// Parses a duration string like "1h", "30m", "2d" into seconds.
fn parse_duration(s: &str) -> Result<u64, GwsError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(GwsError::Validation("Empty duration".to_string()));
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: u64 = num_str
        .parse()
        .map_err(|_| GwsError::Validation(format!("Invalid duration: {s}")))?;

    match unit {
        "s" => Ok(num),
        "m" => Ok(num * 60),
        "h" => Ok(num * 3600),
        "d" => Ok(num * 86400),
        _ => Err(GwsError::Validation(format!(
            "Unknown duration unit '{unit}'. Use s, m, h, or d."
        ))),
    }
}

/// Parse an RFC 3339 timestamp to Unix seconds.
fn parse_rfc3339_rough(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_matches_renew(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("name").long("name"))
            .arg(Arg::new("all").long("all").action(ArgAction::SetTrue))
            .arg(Arg::new("within").long("within").default_value("1h"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("1h").unwrap(), 3600);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("30m").unwrap(), 1800);
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration("1d").unwrap(), 86400);
        assert_eq!(parse_duration("7d").unwrap(), 604800);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn test_parse_rfc3339_rough() {
        // 2026-02-13T10:00:00Z
        let ts = parse_rfc3339_rough("2026-02-13T10:00:00Z").unwrap();
        assert!(ts > 0);

        // Check simple calculation logic (not verifying exact epoch seconds against a full library)
        // just consistency
        let ts2 = parse_rfc3339_rough("2026-02-13T10:00:01Z").unwrap();
        assert_eq!(ts2, ts + 1);
    }

    #[test]
    fn test_parse_renew_args_name() {
        let matches = make_matches_renew(&["test", "--name", "subs/123"]);
        let config = parse_renew_args(&matches).unwrap();
        assert_eq!(config.name, Some("subs/123".to_string()));
        assert!(!config.all);
    }

    #[test]
    fn test_parse_renew_args_all() {
        let matches = make_matches_renew(&["test", "--all", "--within", "2h"]);
        let config = parse_renew_args(&matches).unwrap();
        assert!(config.name.is_none());
        assert!(config.all);
        assert_eq!(config.within, "2h");
    }

    #[test]
    fn test_parse_renew_args_missing() {
        let matches = make_matches_renew(&["test"]);
        assert!(parse_renew_args(&matches).is_err());
    }

    #[test]
    fn test_filter_subscriptions_to_renew() {
        // Let's use `parse_rfc3339_rough` to get a baseline
        let base_ts = parse_rfc3339_rough("2026-02-13T10:00:00Z").unwrap();

        // sub1 expires in 30m (1800s from base)
        let sub1 = json!({
            "name": "subs/1",
            "expireTime": "2026-02-13T10:30:00Z"
        });

        // sub2 expires in 2h (7200s from base)
        let sub2 = json!({
            "name": "subs/2",
            "expireTime": "2026-02-13T12:00:00Z"
        });

        let subs = vec![sub1, sub2];

        // within 1h (3600s) -> should catch sub1 (1800s < 3600s) but not sub2 (7200s > 3600s)
        let to_renew = filter_subscriptions_to_renew(&subs, base_ts, 3600);

        assert_eq!(to_renew.len(), 1);
        assert_eq!(to_renew[0], "subs/1");

        // within 3h (10800s) -> should catch both
        let to_renew_all = filter_subscriptions_to_renew(&subs, base_ts, 10800);
        assert_eq!(to_renew_all.len(), 2);
    }
}

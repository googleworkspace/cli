use super::*;

/// Handles the `+watch` command — Gmail push notifications via Pub/Sub.
pub(super) async fn handle_watch(
    matches: &ArgMatches,
    sanitize_config: &crate::helpers::modelarmor::SanitizeConfig,
) -> Result<(), GwsError> {
    let config = parse_watch_args(matches);

    if let Some(ref dir) = config.output_dir {
        std::fs::create_dir_all(dir).context("Failed to create output dir")?;
    }

    let client = crate::client::build_client()?;

    // Get tokens
    let gmail_token = auth::get_token(&[GMAIL_SCOPE])
        .await
        .context("Failed to get Gmail token")?;
    let pubsub_token = auth::get_token(&[PUBSUB_SCOPE])
        .await
        .context("Failed to get Pub/Sub token")?;

    let (pubsub_subscription, topic_name, created_resources) = if let Some(ref sub_name) =
        config.subscription
    {
        (sub_name.clone(), None, false)
    } else {
        let project = config
            .project.clone()
            .or_else(|| std::env::var("GOOGLE_WORKSPACE_PROJECT_ID").ok())
            .ok_or_else(|| {
                GwsError::Validation(
                    "--project is required when not using --subscription (or set GOOGLE_WORKSPACE_PROJECT_ID)".to_string(),
                )
            })?;

        let suffix = format!("{:08x}", rand::random::<u32>());
        let topic = if let Some(ref t) = config.topic {
            t.clone()
        } else {
            let t = format!("projects/{project}/topics/gws-gmail-watch-{suffix}");
            // Create Pub/Sub topic
            eprintln!("Creating Pub/Sub topic: {t}");
            let resp = client
                .put(format!("https://pubsub.googleapis.com/v1/{t}"))
                .bearer_auth(&pubsub_token)
                .header("Content-Type", "application/json")
                .body("{}")
                .send()
                .await
                .context("Failed to create topic")?;

            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(GwsError::Api {
                    code: 400,
                    message: format!("Failed to create Pub/Sub topic: {body}"),
                    reason: "pubsubError".to_string(),
                });
            }

            // Grant Gmail publish permission on the topic
            eprintln!("Granting Gmail push permission on topic...");
            let iam_body = json!({
                "policy": {
                    "bindings": [{
                        "role": "roles/pubsub.publisher",
                        "members": ["serviceAccount:gmail-api-push@system.gserviceaccount.com"]
                    }]
                }
            });
            let resp = client
                .post(format!("https://pubsub.googleapis.com/v1/{t}:setIamPolicy"))
                .bearer_auth(&pubsub_token)
                .header("Content-Type", "application/json")
                .json(&iam_body)
                .send()
                .await
                .context("Failed to set topic IAM policy")?;

            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                eprintln!("Warning: Could not auto-grant Gmail push permission.");
                eprintln!("You may need to manually grant publisher access:");
                eprintln!(
                    "  gcloud pubsub topics add-iam-policy-binding {} \\",
                    t.split('/').rfind(|s| !s.is_empty()).unwrap_or(&t)
                );
                eprintln!(
                    "    --member=serviceAccount:gmail-api-push@system.gserviceaccount.com \\"
                );
                eprintln!("    --role=roles/pubsub.publisher");
                eprintln!("Error: {body}");
            }

            t
        };

        let sub = format!("projects/{project}/subscriptions/gws-gmail-watch-{suffix}");

        // 3. Create Pub/Sub subscription
        eprintln!("Creating Pub/Sub subscription: {sub}");
        let sub_body = json!({
            "topic": topic,
            "ackDeadlineSeconds": 60,
        });
        let resp = client
            .put(format!("https://pubsub.googleapis.com/v1/{sub}"))
            .bearer_auth(&pubsub_token)
            .header("Content-Type", "application/json")
            .json(&sub_body)
            .send()
            .await
            .context("Failed to create subscription")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GwsError::Api {
                code: 400,
                message: format!("Failed to create Pub/Sub subscription: {body}"),
                reason: "pubsubError".to_string(),
            });
        }

        // 4. Call gmail.users.watch
        eprintln!("Setting up Gmail watch...");
        let mut watch_body = json!({
            "topicName": topic,
        });
        if let Some(ref label_ids) = config.label_ids {
            let labels: Vec<&str> = label_ids.split(',').map(|s| s.trim()).collect();
            watch_body["labelIds"] = json!(labels);
        }

        let resp = client
            .post("https://gmail.googleapis.com/gmail/v1/users/me/watch")
            .bearer_auth(&gmail_token)
            .header("Content-Type", "application/json")
            .json(&watch_body)
            .send()
            .await
            .context("Failed to call gmail.users.watch")?;

        let watch_resp: Value = resp
            .json()
            .await
            .context("Failed to parse watch response")?;

        if let Some(err) = watch_resp.get("error") {
            return Err(GwsError::Api {
                code: err.get("code").and_then(|c| c.as_u64()).unwrap_or(400) as u16,
                message: format!(
                    "gmail.users.watch failed: {}",
                    serde_json::to_string(err).unwrap_or_default()
                ),
                reason: "gmailError".to_string(),
            });
        }

        let history_id = watch_resp
            .get("historyId")
            .and_then(|h| h.as_str())
            .unwrap_or("0");
        let expiration = watch_resp
            .get("expiration")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown");

        eprintln!("Gmail watch active (historyId: {history_id}, expires: {expiration})");
        eprintln!("Listening for new emails...\n");

        (sub, Some(topic), true)
    };

    // Get initial historyId for tracking
    let profile_resp = client
        .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
        .bearer_auth(&gmail_token)
        .send()
        .await
        .context("Failed to get Gmail profile")?;

    let profile: Value = profile_resp.json().await.unwrap_or(json!({}));
    let mut last_history_id: u64 = profile
        .get("historyId")
        .and_then(|h| h.as_str().or_else(|| h.as_u64().map(|_| "")))
        .and_then(|s| s.parse().ok())
        .or_else(|| profile.get("historyId").and_then(|h| h.as_u64()))
        .unwrap_or(0);

    // Pull loop
    let result = watch_pull_loop(
        &client,
        &pubsub_token,
        &gmail_token,
        &pubsub_subscription,
        &mut last_history_id,
        config.clone(),
        sanitize_config,
    )
    .await;

    // Cleanup or print reconnection info
    if created_resources {
        if config.cleanup {
            eprintln!("\nCleaning up Pub/Sub resources...");
            let _ = client
                .delete(format!(
                    "https://pubsub.googleapis.com/v1/{pubsub_subscription}"
                ))
                .bearer_auth(&pubsub_token)
                .send()
                .await;
            if let Some(ref topic) = topic_name {
                let _ = client
                    .delete(format!("https://pubsub.googleapis.com/v1/{topic}"))
                    .bearer_auth(&pubsub_token)
                    .send()
                    .await;
            }
            eprintln!("Cleanup complete.");
        } else {
            eprintln!("\n--- Reconnection Info ---");
            eprintln!(
                "To reconnect later:\n  gws gmail +watch --subscription {}",
                pubsub_subscription
            );
            if let Some(ref topic) = topic_name {
                eprintln!("Pub/Sub topic: {topic}");
            }
            eprintln!("Pub/Sub subscription: {pubsub_subscription}");
            eprintln!("Note: Gmail watch expires after 7 days. Re-run +watch to renew.");
        }
    }

    result
}

/// Pull loop for Gmail watch — polls Pub/Sub, fetches messages via history API.
async fn watch_pull_loop(
    client: &reqwest::Client,
    pubsub_token: &str,
    gmail_token: &str,
    subscription: &str,
    last_history_id: &mut u64,
    config: WatchConfig,
    sanitize_config: &crate::helpers::modelarmor::SanitizeConfig,
) -> Result<(), GwsError> {
    loop {
        let pull_body = json!({ "maxMessages": config.max_messages });
        let pull_future = client
            .post(format!(
                "https://pubsub.googleapis.com/v1/{subscription}:pull"
            ))
            .bearer_auth(pubsub_token)
            .header("Content-Type", "application/json")
            .json(&pull_body)
            .timeout(std::time::Duration::from_secs(config.poll_interval.max(10)))
            .send();

        let resp = tokio::select! {
            result = pull_future => {
                match result {
                    Ok(r) => r,
                    Err(e) if e.is_timeout() => continue,
                    Err(e) => return Err(GwsError::Other(anyhow::anyhow!("Pub/Sub pull failed: {e}"))),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nReceived interrupt, stopping...");
                return Ok(());
            }
        };

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GwsError::Api {
                code: 400,
                message: format!("Pub/Sub pull failed: {body}"),
                reason: "pubsubError".to_string(),
            });
        }

        let pull_response: Value = resp.json().await.context("Failed to parse pull response")?;

        let (ack_ids, max_history_id) = process_pull_response(&pull_response);

        if max_history_id > *last_history_id && *last_history_id > 0 {
            // Fetch new messages via history API
            fetch_and_output_messages(
                client,
                gmail_token,
                *last_history_id,
                &config.format,
                config.output_dir.as_deref(),
                sanitize_config,
            )
            .await?;
        }

        if max_history_id > *last_history_id {
            *last_history_id = max_history_id;
        }

        // Acknowledge messages
        if !ack_ids.is_empty() {
            let ack_body = json!({ "ackIds": ack_ids });
            let _ = client
                .post(format!(
                    "https://pubsub.googleapis.com/v1/{subscription}:acknowledge"
                ))
                .bearer_auth(pubsub_token)
                .header("Content-Type", "application/json")
                .json(&ack_body)
                .send()
                .await;
        }

        if config.once {
            break;
        }

        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(config.poll_interval)) => {},
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nReceived interrupt, stopping...");
                break;
            }
        }
    }

    Ok(())
}

fn process_pull_response(response: &Value) -> (Vec<String>, u64) {
    let mut ack_ids = Vec::new();
    let mut max_history_id = 0;

    if let Some(messages) = response.get("receivedMessages").and_then(|m| m.as_array()) {
        for msg in messages {
            if let Some(ack_id) = msg.get("ackId").and_then(|a| a.as_str()) {
                ack_ids.push(ack_id.to_string());
            }

            // Extract historyId from the notification
            if let Some(pubsub_msg) = msg.get("message") {
                let data = pubsub_msg
                    .get("data")
                    .and_then(|d| d.as_str())
                    .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok());

                if let Some(notification) = data {
                    let notif_history_id = notification
                        .get("historyId")
                        .and_then(|h| h.as_u64().or_else(|| h.as_str()?.parse().ok()))
                        .unwrap_or(0);

                    if notif_history_id > max_history_id {
                        max_history_id = notif_history_id;
                    }
                }
            }
        }
    }

    (ack_ids, max_history_id)
}

/// Fetches new messages since `start_history_id` and outputs them as NDJSON.
async fn fetch_and_output_messages(
    client: &reqwest::Client,
    gmail_token: &str,
    start_history_id: u64,
    msg_format: &str,
    output_dir: Option<&str>,
    sanitize_config: &crate::helpers::modelarmor::SanitizeConfig,
) -> Result<(), GwsError> {
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/history?startHistoryId={}&historyTypes=messageAdded",
        start_history_id
    );

    let resp = client
        .get(&url)
        .bearer_auth(gmail_token)
        .send()
        .await
        .context("Failed to get history")?;

    let body: Value = resp.json().await.unwrap_or(json!({}));

    let msg_ids = extract_message_ids_from_history(&body);

    for msg_id in msg_ids {
        // Fetch full message
        let msg_url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format={}",
            msg_id, msg_format
        );
        let msg_resp = client.get(&msg_url).bearer_auth(gmail_token).send().await;

        if let Ok(resp) = msg_resp {
            if let Ok(mut full_msg) = resp.json::<Value>().await {
                // Apply sanitization if configured
                if let Some(ref template) = sanitize_config.template {
                    let text_to_check = serde_json::to_string(&full_msg).unwrap_or_default();
                    match crate::helpers::modelarmor::sanitize_text(template, &text_to_check).await
                    {
                        Ok(result) => {
                            if let Some(sanitized_msg) = apply_sanitization_result(
                                full_msg,
                                sanitize_config,
                                &result,
                                &msg_id,
                            ) {
                                full_msg = sanitized_msg;
                            } else {
                                continue;
                            }
                        }
                        Err(e) => {
                            eprintln!("\x1b[33m[WARNING]\x1b[0m Model Armor sanitization failed for message {msg_id}: {e}");
                        }
                    }
                }

                let json_str =
                    serde_json::to_string_pretty(&full_msg).unwrap_or_else(|_| "{}".to_string());
                if let Some(dir) = output_dir {
                    let path = format!("{dir}/{msg_id}.json");
                    if let Err(e) = std::fs::write(&path, &json_str) {
                        eprintln!("Warning: failed to write {path}: {e}");
                    } else {
                        eprintln!("Wrote {path}");
                    }
                } else {
                    println!(
                        "{}",
                        serde_json::to_string(&full_msg).unwrap_or_else(|_| "{}".to_string())
                    );
                }
            }
        }
    }

    Ok(())
}

fn apply_sanitization_result(
    mut full_msg: Value,
    sanitize_config: &crate::helpers::modelarmor::SanitizeConfig,
    result: &crate::helpers::modelarmor::SanitizationResult,
    msg_id: &str,
) -> Option<Value> {
    if result.filter_match_state == "MATCH_FOUND" {
        match sanitize_config.mode {
            crate::helpers::modelarmor::SanitizeMode::Block => {
                eprintln!(
                    "\x1b[31m[BLOCKED]\x1b[0m Message {msg_id} blocked by Model Armor (match found)"
                );
                return None;
            }
            crate::helpers::modelarmor::SanitizeMode::Warn => {
                eprintln!("\x1b[33m[WARNING]\x1b[0m Model Armor match found in message {msg_id}");
                full_msg["_sanitization"] = serde_json::json!({
                    "filterMatchState": result.filter_match_state,
                    "filterResults": result.filter_results,
                });
            }
        }
    }
    Some(full_msg)
}

fn extract_message_ids_from_history(history_body: &Value) -> Vec<String> {
    let mut seen_ids = std::collections::HashSet::new();
    let mut result = Vec::new();

    if let Some(history) = history_body.get("history").and_then(|h| h.as_array()) {
        for entry in history {
            if let Some(added) = entry.get("messagesAdded").and_then(|m| m.as_array()) {
                for msg_entry in added {
                    if let Some(msg_id) = msg_entry
                        .get("message")
                        .and_then(|m| m.get("id"))
                        .and_then(|id| id.as_str())
                    {
                        if seen_ids.insert(msg_id.to_string()) {
                            result.push(msg_id.to_string());
                        }
                    }
                }
            }
        }
    }
    result
}

#[derive(Clone)]
struct WatchConfig {
    project: Option<String>,
    subscription: Option<String>,
    topic: Option<String>,
    label_ids: Option<String>,
    max_messages: u32,
    poll_interval: u64,
    format: String,
    once: bool,
    cleanup: bool,
    output_dir: Option<String>,
}

fn parse_watch_args(matches: &ArgMatches) -> WatchConfig {
    WatchConfig {
        project: matches.get_one::<String>("project").cloned(),
        subscription: matches.get_one::<String>("subscription").cloned(),
        topic: matches.get_one::<String>("topic").cloned(),
        label_ids: matches.get_one::<String>("label-ids").cloned(),
        max_messages: matches
            .get_one::<String>("max-messages")
            .and_then(|s| s.parse().ok())
            .unwrap_or(10),
        poll_interval: matches
            .get_one::<String>("poll-interval")
            .and_then(|s| s.parse().ok())
            .unwrap_or(5),
        format: matches
            .get_one::<String>("msg-format")
            .map(|s| s.as_str())
            .unwrap_or("full")
            .to_string(),
        once: matches.get_flag("once"),
        cleanup: matches.get_flag("cleanup"),
        output_dir: matches.get_one::<String>("output-dir").cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_message_ids_from_history() {
        let history = json!({
            "history": [
                {
                    "messagesAdded": [
                        { "message": { "id": "msg1", "threadId": "t1" } }
                    ]
                },
                {
                    "messagesAdded": [
                        { "message": { "id": "msg2", "threadId": "t2" } },
                        { "message": { "id": "msg1", "threadId": "t1" } } // duplicate
                    ]
                }
            ]
        });

        let ids = extract_message_ids_from_history(&history);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"msg1".to_string()));
        assert!(ids.contains(&"msg2".to_string()));
    }

    #[test]
    fn test_extract_message_ids_empty() {
        let history = json!({});
        let ids = extract_message_ids_from_history(&history);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_process_pull_response() {
        let encoded_data = URL_SAFE
            .encode(json!({ "emailAddress": "me@example.com", "historyId": 12345 }).to_string());
        let response = json!({
            "receivedMessages": [
                {
                    "ackId": "ack1",
                    "message": {
                        "data": encoded_data,
                        "messageId": "msg1"
                    }
                },
                {
                    "ackId": "ack2",
                    "message": {
                        "data": URL_SAFE.encode(json!({ "historyId": 100 }).to_string()),
                        "messageId": "msg2"
                    }
                }
            ]
        });

        let (ack_ids, max_history) = process_pull_response(&response);
        assert_eq!(ack_ids.len(), 2);
        assert!(ack_ids.contains(&"ack1".to_string()));
        assert!(ack_ids.contains(&"ack2".to_string()));
        assert_eq!(max_history, 12345);
    }

    fn make_matches_watch(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("project").long("project"))
            .arg(Arg::new("subscription").long("subscription"))
            .arg(Arg::new("topic").long("topic"))
            .arg(Arg::new("label-ids").long("label-ids"))
            .arg(Arg::new("max-messages").long("max-messages"))
            .arg(Arg::new("poll-interval").long("poll-interval"))
            .arg(Arg::new("msg-format").long("msg-format"))
            .arg(Arg::new("once").long("once").action(ArgAction::SetTrue))
            .arg(
                Arg::new("cleanup")
                    .long("cleanup")
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("output-dir").long("output-dir"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_watch_args() {
        let matches = make_matches_watch(&[
            "test",
            "--project",
            "p1",
            "--subscription",
            "s1",
            "--max-messages",
            "20",
            "--once",
        ]);
        let config = parse_watch_args(&matches);
        assert_eq!(config.project.unwrap(), "p1");
        assert_eq!(config.subscription.unwrap(), "s1");
        assert_eq!(config.max_messages, 20);
        assert!(config.once);
        assert!(!config.cleanup);
        // Default check handled by unwrap_or
        assert_eq!(config.poll_interval, 5);
        assert_eq!(config.format, "full");
        assert_eq!(config.label_ids, None);
        assert_eq!(config.topic, None);
        assert_eq!(config.output_dir, None);
    }

    #[test]
    fn test_parse_watch_args_defaults() {
        let matches = make_matches_watch(&["test"]);
        let config = parse_watch_args(&matches);
        assert_eq!(config.project, None);
        assert_eq!(config.subscription, None);
        assert_eq!(config.max_messages, 10);
        assert_eq!(config.poll_interval, 5);
        assert_eq!(config.format, "full");
        assert!(!config.once);
        assert!(!config.cleanup);
    }

    #[test]
    fn test_parse_watch_args_invalid_numbers() {
        let matches = make_matches_watch(&[
            "test",
            "--max-messages",
            "not_a_number",
            "--poll-interval",
            "invalid",
        ]);
        let config = parse_watch_args(&matches);
        // Should fallback to defaults
        assert_eq!(config.max_messages, 10);
        assert_eq!(config.poll_interval, 5);
    }

    #[test]
    fn test_apply_sanitization_result_block_mode() {
        let msg = json!({ "id": "msg1" });
        let config = crate::helpers::modelarmor::SanitizeConfig {
            template: Some("projects/x/locations/y/templates/z".to_string()),
            mode: crate::helpers::modelarmor::SanitizeMode::Block,
        };
        let result = crate::helpers::modelarmor::SanitizationResult {
            filter_match_state: "MATCH_FOUND".to_string(),
            filter_results: json!([]),
            invocation_result: "{}".to_string(),
        };

        let output = apply_sanitization_result(msg, &config, &result, "msg1");
        assert!(output.is_none());
    }

    #[test]
    fn test_apply_sanitization_result_warn_mode() {
        let msg = json!({ "id": "msg1" });
        let config = crate::helpers::modelarmor::SanitizeConfig {
            template: Some("projects/x/locations/y/templates/z".to_string()),
            mode: crate::helpers::modelarmor::SanitizeMode::Warn,
        };
        let result = crate::helpers::modelarmor::SanitizationResult {
            filter_match_state: "MATCH_FOUND".to_string(),
            filter_results: json!([]),
            invocation_result: "{}".to_string(),
        };

        let output = apply_sanitization_result(msg, &config, &result, "msg1").unwrap();
        // Warn mode adds the `_sanitization` metadata.
        assert!(output.get("_sanitization").is_some());
        assert_eq!(output["_sanitization"]["filterMatchState"], "MATCH_FOUND");
    }

    #[test]
    fn test_apply_sanitization_result_no_match() {
        let msg = json!({ "id": "msg1" });
        let config = crate::helpers::modelarmor::SanitizeConfig {
            template: Some("projects/x/locations/y/templates/z".to_string()),
            mode: crate::helpers::modelarmor::SanitizeMode::Block,
        };
        let result = crate::helpers::modelarmor::SanitizationResult {
            filter_match_state: "NO_MATCH_FOUND".to_string(),
            filter_results: json!([]),
            invocation_result: "{}".to_string(),
        };

        let output = apply_sanitization_result(msg.clone(), &config, &result, "msg1").unwrap();
        // If no match found, block mode returns the exact input untouched.
        assert_eq!(output, msg);
        assert!(output.get("_sanitization").is_none());
    }
}

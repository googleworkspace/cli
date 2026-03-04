use super::*;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Builder)]
#[builder(setter(into))]
pub struct SubscribeConfig {
    #[builder(default)]
    target: Option<String>,
    #[builder(default)]
    event_types: Vec<String>,
    #[builder(default)]
    project: Option<ProjectId>,
    #[builder(default)]
    subscription: Option<SubscriptionName>,
    #[builder(default = "10")]
    max_messages: u32,
    #[builder(default = "2")]
    poll_interval: u64,
    #[builder(default)]
    once: bool,
    #[builder(default)]
    cleanup: bool,
    #[builder(default)]
    no_ack: bool,
    #[builder(default)]
    output_dir: Option<PathBuf>,
}

fn parse_subscribe_args(matches: &ArgMatches) -> Result<SubscribeConfig, GwsError> {
    let mut builder = SubscribeConfigBuilder::default();

    if let Some(target) = matches.get_one::<String>("target") {
        builder.target(Some(target.clone()));
    }
    if let Some(event_types) = matches.get_one::<String>("event-types") {
        builder.event_types(
            event_types
                .split(',')
                .map(|t| t.trim().to_string())
                .collect::<Vec<_>>(),
        );
    }
    if let Some(project) = matches
        .get_one::<String>("project")
        .cloned()
        .or_else(|| std::env::var("GOOGLE_WORKSPACE_PROJECT_ID").ok())
    {
        builder.project(Some(ProjectId(project)));
    }
    if let Some(subscription) = matches.get_one::<String>("subscription") {
        crate::validate::validate_resource_name(subscription)?;
        builder.subscription(Some(SubscriptionName(subscription.clone())));
    }
    if let Some(max_messages) = matches
        .get_one::<String>("max-messages")
        .and_then(|s| s.parse::<u32>().ok())
    {
        builder.max_messages(max_messages);
    }
    if let Some(poll_interval) = matches
        .get_one::<String>("poll-interval")
        .and_then(|s| s.parse::<u64>().ok())
    {
        builder.poll_interval(poll_interval);
    }
    builder.once(matches.get_flag("once"));
    builder.cleanup(matches.get_flag("cleanup"));
    builder.no_ack(matches.get_flag("no-ack"));
    if let Some(output_dir) = matches.get_one::<String>("output-dir") {
        builder.output_dir(Some(crate::validate::validate_safe_output_dir(output_dir)?));
    }

    let config = builder
        .build()
        .map_err(|e| GwsError::Validation(e.to_string()))?;
    validate_subscribe_config(&config)?;
    Ok(config)
}

fn validate_subscribe_config(config: &SubscribeConfig) -> Result<(), GwsError> {
    if config.subscription.is_none() {
        if config.target.is_none() {
            return Err(GwsError::Validation(
                "--target is required when not using --subscription".to_string(),
            ));
        }
        if config.event_types.is_empty() {
            return Err(GwsError::Validation(
                "--event-types is required when not using --subscription".to_string(),
            ));
        }
        if config.project.is_none() {
            return Err(GwsError::Validation(
                "--project is required when not using --subscription (or set GOOGLE_WORKSPACE_PROJECT_ID)".to_string(),
            ));
        }
    }
    Ok(())
}

/// Handles the `+subscribe` command.
pub(super) async fn handle_subscribe(
    _doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let config = parse_subscribe_args(matches)?;

    if let Some(ref dir) = config.output_dir {
        std::fs::create_dir_all(dir).context("Failed to create output dir")?;
    }

    let client = crate::client::build_client()?;

    // Get Pub/Sub token
    let pubsub_token = auth::get_token(&[PUBSUB_SCOPE])
        .await
        .map_err(|e| GwsError::Auth(format!("Failed to get Pub/Sub token: {e}")))?;

    let (pubsub_subscription, topic_name, ws_subscription_name, created_resources) =
        if let Some(ref sub_name) = config.subscription {
            // Use existing subscription — no setup needed
            (sub_name.0.clone(), None, None, false)
        } else {
            // Full setup: create Pub/Sub topic + subscription + Workspace Events subscription
            let target = config.target.clone().unwrap();
            let project =
                crate::validate::validate_resource_name(&config.project.clone().unwrap().0)?
                    .to_string();
            let event_types_str: Vec<&str> =
                config.event_types.iter().map(|s| s.as_str()).collect();

            // Generate descriptive names from event types
            // e.g. "google.workspace.drive.file.v1.updated" -> "drive-file-updated"
            let slug = derive_slug_from_event_types(&event_types_str);
            let suffix = format!("{:08x}", rand::random::<u32>());
            let topic = format!("projects/{project}/topics/gws-{slug}-{suffix}");
            let sub = format!("projects/{project}/subscriptions/gws-{slug}-{suffix}");

            // 1. Create Pub/Sub topic
            eprintln!("Creating Pub/Sub topic: {topic}");
            let resp = client
                .put(format!("https://pubsub.googleapis.com/v1/{topic}"))
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
                    enable_url: None,
                });
            }

            // 2. Create Pub/Sub subscription
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
                    enable_url: None,
                });
            }

            // 3. Create Workspace Events subscription
            eprintln!("Creating Workspace Events subscription...");
            let ws_token = auth::get_token(&[WORKSPACE_EVENTS_SCOPE])
                .await
                .map_err(|e| {
                    GwsError::Auth(format!("Failed to get Workspace Events token: {e}"))
                })?;

            let ws_body = json!({
                "targetResource": target,
                "eventTypes": config.event_types,
                "notificationEndpoint": {
                    "pubsubTopic": topic,
                },
                "payloadOptions": {
                    "includeResource": true,
                },
            });

            let resp = client
                .post("https://workspaceevents.googleapis.com/v1/subscriptions")
                .bearer_auth(&ws_token)
                .header("Content-Type", "application/json")
                .json(&ws_body)
                .send()
                .await
                .context("Failed to create Workspace Events subscription")?;

            let resp_body: Value = resp
                .json()
                .await
                .context("Failed to parse subscription response")?;

            let ws_sub_name = resp_body
                // Direct subscription response
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| s.starts_with("subscriptions/"))
                .or_else(|| {
                    // LRO response — check response.name
                    resp_body
                        .get("response")
                        .and_then(|r| r.get("name"))
                        .and_then(|v| v.as_str())
                })
                .or_else(|| {
                    // LRO response — check metadata.subscription
                    resp_body
                        .get("metadata")
                        .and_then(|m| m.get("subscription"))
                        .and_then(|v| v.as_str())
                })
                .or_else(|| {
                    // Fall back to the operation name itself
                    resp_body.get("name").and_then(|v| v.as_str())
                })
                .unwrap_or("pending")
                .to_string();

            eprintln!("Workspace Events subscription: {ws_sub_name}");
            eprintln!("Listening for events...\n");

            (sub, Some(topic), Some(ws_sub_name), true)
        };

    // Pull loop
    let result = pull_loop(&client, &pubsub_token, &pubsub_subscription, config.clone()).await;

    // On exit, print reconnection info or cleanup
    if created_resources {
        if config.cleanup {
            eprintln!("\nCleaning up Pub/Sub resources...");
            // Delete Pub/Sub subscription
            let _ = client
                .delete(format!(
                    "https://pubsub.googleapis.com/v1/{pubsub_subscription}"
                ))
                .bearer_auth(&pubsub_token)
                .send()
                .await;
            // Delete Pub/Sub topic
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
                "To reconnect later:\n  gws events +subscribe --subscription {}",
                pubsub_subscription
            );
            if let Some(ref ws_name) = ws_subscription_name {
                eprintln!("Workspace Events subscription: {ws_name}");
            }
            if let Some(ref topic) = topic_name {
                eprintln!("Pub/Sub topic: {topic}");
            }
            eprintln!("Pub/Sub subscription: {pubsub_subscription}");
            eprintln!("To clean up manually:");
            if let Some(ref topic) = topic_name {
                eprintln!(
                    "  gcloud pubsub subscriptions delete {}",
                    pubsub_subscription
                );
                eprintln!("  gcloud pubsub topics delete {topic}");
            }
        }
    }

    result
}

/// Pulls messages from a Pub/Sub subscription in a loop.
async fn pull_loop(
    client: &reqwest::Client,
    token: &str,
    subscription: &str,
    config: SubscribeConfig,
) -> Result<(), GwsError> {
    let mut file_counter: u64 = 0;
    loop {
        let pull_body = json!({
            "maxMessages": config.max_messages,
        });

        let pull_future = client
            .post(format!(
                "https://pubsub.googleapis.com/v1/{subscription}:pull"
            ))
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .json(&pull_body)
            .timeout(std::time::Duration::from_secs(config.poll_interval.max(10)))
            .send();

        let resp = tokio::select! {
            result = pull_future => {
                match result {
                    Ok(r) => r,
                    Err(e) if e.is_timeout() => continue,
                    Err(e) => return Err(anyhow::anyhow!("Pub/Sub pull failed: {e}").into()),
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
                enable_url: None,
            });
        }

        let pull_response: Value = resp.json().await.context("Failed to parse pull response")?;

        let (ack_ids, events) = process_events_pull_response(&pull_response);

        for event in events {
            let json_str =
                serde_json::to_string_pretty(&event).unwrap_or_else(|_| "{}".to_string());
            if let Some(ref dir) = config.output_dir {
                file_counter += 1;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let path = dir.join(format!("{ts}_{file_counter}.json"));
                if let Err(e) = std::fs::write(&path, &json_str) {
                    eprintln!("Warning: failed to write {}: {e}", path.display());
                } else {
                    eprintln!("Wrote {}", path.display());
                }
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string())
                );
            }
        }

        // Acknowledge messages
        if !config.no_ack && !ack_ids.is_empty() {
            let ack_body = json!({
                "ackIds": ack_ids,
            });

            let _ = client
                .post(format!(
                    "https://pubsub.googleapis.com/v1/{subscription}:acknowledge"
                ))
                .bearer_auth(token)
                .header("Content-Type", "application/json")
                .json(&ack_body)
                .send()
                .await;
        }

        if config.once {
            break;
        }

        // Check for SIGINT between polls
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

fn process_events_pull_response(response: &Value) -> (Vec<String>, Vec<Value>) {
    let mut ack_ids = Vec::new();
    let mut events = Vec::new();

    if let Some(messages) = response.get("receivedMessages").and_then(|m| m.as_array()) {
        for msg in messages {
            if let Some(ack_id) = msg.get("ackId").and_then(|a| a.as_str()) {
                ack_ids.push(ack_id.to_string());
            }

            if let Some(pubsub_msg) = msg.get("message") {
                events.push(decode_cloud_event(pubsub_msg));
            }
        }
    }

    (ack_ids, events)
}

/// Decodes a Pub/Sub message containing a CloudEvent.
fn decode_cloud_event(pubsub_msg: &Value) -> Value {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let attributes = pubsub_msg.get("attributes").cloned().unwrap_or(json!({}));

    let event_type = attributes
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let source = attributes
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let time = attributes
        .get("time")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Decode base64 data
    let data = pubsub_msg
        .get("data")
        .and_then(|d| d.as_str())
        .and_then(|d| STANDARD.decode(d).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(json!(null));

    json!({
        "type": event_type,
        "source": source,
        "time": time,
        "attributes": attributes,
        "data": data,
    })
}

/// Derives a readable slug from event types for Pub/Sub resource naming.
/// e.g. ["google.workspace.drive.file.v1.updated"] -> "drive-file-updated"
/// Multiple types are joined: ["...drive.file.v1.updated", "...drive.file.v1.created"] -> "drive-file-updated-created"
fn derive_slug_from_event_types(event_types: &[&str]) -> String {
    let parts: Vec<String> = event_types
        .iter()
        .map(|et| {
            // Strip "google.workspace." prefix and version segment
            let stripped = et.strip_prefix("google.workspace.").unwrap_or(et);
            // Split by '.', remove version-like segments (e.g. "v1")
            let segments: Vec<&str> = stripped
                .split('.')
                .filter(|s| {
                    !s.starts_with('v')
                        || s.len() > 3
                        || !s[1..].chars().all(|c| c.is_ascii_digit())
                })
                .collect();
            segments.join("-")
        })
        .collect();

    let slug = if parts.len() == 1 {
        parts[0].clone()
    } else {
        // Find common prefix across event types, then append distinct suffixes
        let first_segments: Vec<&str> = parts[0].split('-').collect();
        let mut common_len = 0;
        'outer: for i in 0..first_segments.len() {
            for p in &parts[1..] {
                let segs: Vec<&str> = p.split('-').collect();
                if i >= segs.len() || segs[i] != first_segments[i] {
                    break 'outer;
                }
            }
            common_len = i + 1;
        }
        let prefix = first_segments[..common_len].join("-");
        let suffixes: Vec<String> = parts
            .iter()
            .map(|p| {
                let segs: Vec<&str> = p.split('-').collect();
                segs[common_len..].join("-")
            })
            .filter(|s| !s.is_empty())
            .collect();

        if suffixes.is_empty() {
            prefix
        } else {
            format!("{}-{}", prefix, suffixes.join("-"))
        }
    };

    // Truncate to keep Pub/Sub resource names within limits
    let slug = if slug.len() > 40 { &slug[..40] } else { &slug };
    slug.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_matches_subscribe(args: &[&str]) -> ArgMatches {
        let cmd = Command::new("test")
            .arg(Arg::new("target").long("target"))
            .arg(Arg::new("event-types").long("event-types"))
            .arg(Arg::new("project").long("project"))
            .arg(Arg::new("subscription").long("subscription"))
            .arg(Arg::new("max-messages").long("max-messages"))
            .arg(Arg::new("poll-interval").long("poll-interval"))
            .arg(Arg::new("once").long("once").action(ArgAction::SetTrue))
            .arg(
                Arg::new("cleanup")
                    .long("cleanup")
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("no-ack").long("no-ack").action(ArgAction::SetTrue))
            .arg(Arg::new("output-dir").long("output-dir"));
        cmd.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_parse_subscribe_args_invalid_output_dir() {
        let matches = make_matches_subscribe(&["test", "--output-dir", "../../etc"]);
        let result = parse_subscribe_args(&matches);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("outside the current directory"));
    }

    #[test]
    fn test_parse_subscribe_args() {
        let matches = make_matches_subscribe(&[
            "test",
            "--target",
            "//chat/spaces/A",
            "--event-types",
            "type1,type2",
            "--project",
            "my-project",
            "--max-messages",
            "20",
            "--once",
        ]);
        let config = parse_subscribe_args(&matches).unwrap();

        assert_eq!(config.target, Some("//chat/spaces/A".to_string()));
        assert_eq!(config.event_types, vec!["type1", "type2"]);
        assert_eq!(config.project, Some(ProjectId("my-project".to_string())));
        assert_eq!(config.max_messages, 20);
        assert!(config.once);
        assert!(!config.cleanup);
    }

    #[test]
    fn test_parse_subscribe_args_subscription() {
        let matches = make_matches_subscribe(&["test", "--subscription", "subs/my-sub"]);
        let config = parse_subscribe_args(&matches).unwrap();

        assert_eq!(
            config.subscription,
            Some(SubscriptionName("subs/my-sub".to_string()))
        );
        // Others defaults
        assert_eq!(config.max_messages, 10);
    }

    #[test]
    fn test_slug_single_event_type() {
        let types = vec!["google.workspace.drive.file.v1.updated"];
        assert_eq!(derive_slug_from_event_types(&types), "drive-file-updated");
    }

    #[test]
    fn test_slug_single_event_type_chat() {
        let types = vec!["google.workspace.chat.message.v1.created"];
        assert_eq!(derive_slug_from_event_types(&types), "chat-message-created");
    }

    #[test]
    fn test_slug_multiple_event_types_common_prefix() {
        let types = vec![
            "google.workspace.drive.file.v1.updated",
            "google.workspace.drive.file.v1.created",
        ];
        let slug = derive_slug_from_event_types(&types);
        assert_eq!(slug, "drive-file-updated-created");
    }

    #[test]
    fn test_slug_non_workspace_prefix() {
        let types = vec!["custom.event.type"];
        let slug = derive_slug_from_event_types(&types);
        assert_eq!(slug, "custom-event-type");
    }

    #[test]
    fn test_slug_truncation() {
        // Very long event type should be truncated to 40 chars
        let types = vec!["google.workspace.very.long.service.name.with.many.segments.v1.updated"];
        let slug = derive_slug_from_event_types(&types);
        assert!(slug.len() <= 40);
    }

    #[test]
    fn test_decode_cloud_event() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        let data = json!({"foo": "bar"}).to_string();
        let encoded = STANDARD.encode(data);

        let msg = json!({
            "attributes": {
                "type": "google.workspace.chat.message.v1.created",
                "source": "//chat.googleapis.com/spaces/AAA",
                "time": "2026-02-13T10:00:00Z"
            },
            "data": encoded
        });

        let event = decode_cloud_event(&msg);

        assert_eq!(event["type"], "google.workspace.chat.message.v1.created");
        assert_eq!(event["source"], "//chat.googleapis.com/spaces/AAA");
        assert_eq!(event["data"]["foo"], "bar");
    }

    #[test]
    fn test_process_events_pull_response() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        // Mock a Pub/Sub response with two messages
        let data1 = json!({"id": "1", "content": "hello"}).to_string();
        let encoded1 = STANDARD.encode(data1);

        let data2 = json!({"id": "2", "content": "world"}).to_string();
        let encoded2 = STANDARD.encode(data2);

        let response = json!({
            "receivedMessages": [
                {
                    "ackId": "ack1",
                    "message": {
                        "attributes": {
                            "type": "google.workspace.chat.message.v1.created",
                            "source": "//chat/spaces/A"
                        },
                        "data": encoded1,
                        "messageId": "msg1"
                    }
                },
                {
                    "ackId": "ack2",
                    "message": {
                        "attributes": {
                            "type": "google.workspace.drive.file.v1.updated",
                            "source": "//drive/files/B"
                        },
                        "data": encoded2,
                        "messageId": "msg2"
                    }
                }
            ]
        });

        let (ack_ids, events) = process_events_pull_response(&response);

        assert_eq!(ack_ids.len(), 2);
        assert_eq!(ack_ids[0], "ack1");
        assert_eq!(ack_ids[1], "ack2");

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]["type"],
            "google.workspace.chat.message.v1.created"
        );
        assert_eq!(events[0]["data"]["id"], "1");

        assert_eq!(events[1]["type"], "google.workspace.drive.file.v1.updated");
        assert_eq!(events[1]["data"]["id"], "2");
    }

    #[test]
    fn test_process_events_pull_response_empty() {
        let response = json!({});
        let (ack_ids, events) = process_events_pull_response(&response);
        assert!(ack_ids.is_empty());
        assert!(events.is_empty());
    }

    #[test]
    fn test_handle_subscribe_validation_missing_target() {
        let config = SubscribeConfigBuilder::default()
            .event_types(vec!["type1".to_string()])
            .project(Some(ProjectId("p1".to_string())))
            .build()
            .unwrap();
        let result = validate_subscribe_config(&config);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("--target is required"));
    }

    #[test]
    fn test_handle_subscribe_validation_missing_events() {
        let config = SubscribeConfigBuilder::default()
            .target(Some("target1".to_string()))
            .project(Some(ProjectId("p1".to_string())))
            .build()
            .unwrap();
        let result = validate_subscribe_config(&config);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("--event-types is required"));
    }

    #[test]
    fn test_handle_subscribe_validation_missing_project() {
        let config = SubscribeConfigBuilder::default()
            .target(Some("target1".to_string()))
            .event_types(vec!["type1".to_string()])
            .build()
            .unwrap();
        let result = validate_subscribe_config(&config);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("--project is required"));
    }
}

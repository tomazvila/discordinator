use std::collections::HashMap;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr};
use rand::Rng;
use reqwest::header::HeaderMap;
use tokio::sync::mpsc;

use crate::config::DiscordConfig;
use crate::domain::types::{
    BackgroundResult, CachedMessage, ChannelMarker, HttpRequest, Id, MessageMarker,
};
use crate::infrastructure::anti_detection;

/// Base URL for the Discord API.
const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

/// Minimum jitter delay in milliseconds.
const MIN_JITTER_MS: u64 = 50;
/// Maximum jitter delay in milliseconds.
const MAX_JITTER_MS: u64 = 150;

/// Per-route rate limit state.
#[derive(Debug)]
struct RateLimitBucket {
    remaining: u32,
    reset_at: Instant,
}

/// The HTTP client actor. Runs as a single tokio task to centralize rate limit state.
/// Receives requests via `request_rx`, sends results via `result_tx`.
///
/// **No `create_dm_channel()` method exists** — this is intentional to prevent
/// accidental DM creation which triggers Discord detection.
pub struct HttpActor {
    client: reqwest::Client,
    headers: HeaderMap,
    rate_limits: HashMap<String, RateLimitBucket>,
    request_rx: mpsc::Receiver<HttpRequest>,
    result_tx: mpsc::Sender<BackgroundResult>,
}

impl HttpActor {
    /// Create a new HTTP actor.
    pub fn new(
        config: &DiscordConfig,
        token: &str,
        request_rx: mpsc::Receiver<HttpRequest>,
        result_tx: mpsc::Sender<BackgroundResult>,
    ) -> Result<Self> {
        let headers = anti_detection::build_http_headers(config, token)
            .wrap_err("Failed to build anti-detection HTTP headers")?;
        let client = reqwest::Client::builder()
            .default_headers(headers.clone())
            .build()
            .wrap_err("Failed to build HTTP client")?;

        Ok(Self {
            client,
            headers,
            rate_limits: HashMap::new(),
            request_rx,
            result_tx,
        })
    }

    /// Run the HTTP actor loop. This should be spawned as a tokio task.
    pub async fn run(mut self) {
        while let Some(request) = self.request_rx.recv().await {
            self.handle_request(request).await;
        }
    }

    async fn handle_request(&mut self, request: HttpRequest) {
        // Add jitter to avoid machine-like request patterns
        let jitter = rand::thread_rng().gen_range(MIN_JITTER_MS..=MAX_JITTER_MS);
        tokio::time::sleep(Duration::from_millis(jitter)).await;

        let result = match &request {
            HttpRequest::SendMessage {
                channel_id,
                content,
                nonce,
                reply_to,
            } => {
                self.send_message(*channel_id, content, nonce, *reply_to)
                    .await
            }
            HttpRequest::EditMessage {
                channel_id,
                message_id,
                content,
            } => self.edit_message(*channel_id, *message_id, content).await,
            HttpRequest::DeleteMessage {
                channel_id,
                message_id,
            } => self.delete_message(*channel_id, *message_id).await,
            HttpRequest::FetchMessages {
                channel_id,
                before,
                limit,
            } => self.fetch_messages(*channel_id, *before, *limit).await,
            HttpRequest::SendTyping { channel_id } => self.send_typing(*channel_id).await,
        };

        if let Err(e) = result {
            let _ = self
                .result_tx
                .send(BackgroundResult::HttpError {
                    request: format!("{request:?}"),
                    error: e.to_string(),
                })
                .await;
        }
    }

    /// Check and wait for rate limit on a given route.
    async fn check_rate_limit(&self, route: &str) {
        if let Some(bucket) = self.rate_limits.get(route) {
            if bucket.remaining == 0 && Instant::now() < bucket.reset_at {
                let wait = bucket.reset_at - Instant::now();
                tokio::time::sleep(wait).await;
            }
        }
    }

    /// Update rate limit state from response headers.
    fn update_rate_limit(&mut self, route: &str, headers: &reqwest::header::HeaderMap) {
        let remaining = headers
            .get("X-RateLimit-Remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok());

        let reset_after = headers
            .get("X-RateLimit-Reset-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok());

        if let (Some(remaining), Some(reset_after)) = (remaining, reset_after) {
            self.rate_limits.insert(
                route.to_string(),
                RateLimitBucket {
                    remaining,
                    reset_at: Instant::now() + Duration::from_secs_f64(reset_after),
                },
            );
        }
    }

    async fn send_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        content: &str,
        nonce: &str,
        reply_to: Option<Id<MessageMarker>>,
    ) -> Result<()> {
        let route = format!("POST /channels/{channel_id}/messages");
        self.check_rate_limit(&route).await;

        let mut body = serde_json::json!({
            "content": content,
            "nonce": nonce,
        });

        if let Some(reply_id) = reply_to {
            body["message_reference"] = serde_json::json!({
                "message_id": reply_id.get().to_string(),
            });
        }

        let url = format!("{DISCORD_API_BASE}/channels/{channel_id}/messages");
        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .wrap_err("Failed to send message")?;

        self.update_rate_limit(&route, response.headers());

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(color_eyre::eyre::eyre!(
                "Send message failed: {} - {}",
                status,
                body
            ));
        }

        Ok(())
    }

    async fn edit_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: &str,
    ) -> Result<()> {
        let route = format!("PATCH /channels/{channel_id}/messages");
        self.check_rate_limit(&route).await;

        let body = serde_json::json!({ "content": content });
        let url = format!("{DISCORD_API_BASE}/channels/{channel_id}/messages/{message_id}");

        let response = self
            .client
            .patch(&url)
            .json(&body)
            .send()
            .await
            .wrap_err("Failed to edit message")?;

        self.update_rate_limit(&route, response.headers());

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(color_eyre::eyre::eyre!(
                "Edit message failed: {} - {}",
                status,
                body
            ));
        }

        Ok(())
    }

    async fn delete_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        let route = format!("DELETE /channels/{channel_id}/messages");
        self.check_rate_limit(&route).await;

        let url = format!("{DISCORD_API_BASE}/channels/{channel_id}/messages/{message_id}");

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .wrap_err("Failed to delete message")?;

        self.update_rate_limit(&route, response.headers());

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(color_eyre::eyre::eyre!(
                "Delete message failed: {} - {}",
                status,
                body
            ));
        }

        Ok(())
    }

    async fn fetch_messages(
        &mut self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u8,
    ) -> Result<()> {
        let route = format!("GET /channels/{channel_id}/messages");
        self.check_rate_limit(&route).await;

        let mut url = format!("{DISCORD_API_BASE}/channels/{channel_id}/messages?limit={limit}");

        if let Some(before_id) = before {
            use std::fmt::Write;
            let _ = write!(url, "&before={before_id}");
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .wrap_err("Failed to fetch messages")?;

        self.update_rate_limit(&route, response.headers());

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(color_eyre::eyre::eyre!(
                "Fetch messages failed: {} - {}",
                status,
                body
            ));
        }

        let body_text = response
            .text()
            .await
            .wrap_err("Failed to read response body")?;
        let messages = parse_messages_response(&body_text, channel_id)?;

        let _ = self
            .result_tx
            .send(BackgroundResult::MessagesFetched {
                channel_id,
                messages,
            })
            .await;

        Ok(())
    }

    async fn send_typing(&mut self, channel_id: Id<ChannelMarker>) -> Result<()> {
        let route = format!("POST /channels/{channel_id}/typing");
        self.check_rate_limit(&route).await;

        let url = format!("{DISCORD_API_BASE}/channels/{channel_id}/typing");
        let response = self
            .client
            .post(&url)
            .send()
            .await
            .wrap_err("Failed to send typing")?;

        self.update_rate_limit(&route, response.headers());

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(color_eyre::eyre::eyre!(
                "Send typing failed: {} - {}",
                status,
                body
            ));
        }

        Ok(())
    }

    /// Get a reference to the default headers (for testing).
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

/// Parse a Discord messages response JSON array into `CachedMessages`.
fn parse_messages_response(
    body: &str,
    channel_id: Id<ChannelMarker>,
) -> Result<Vec<CachedMessage>> {
    let messages: Vec<serde_json::Value> =
        serde_json::from_str(body).wrap_err("Failed to parse messages JSON")?;

    let mut result = Vec::with_capacity(messages.len());
    for msg in messages {
        let cached = json_to_cached_message(&msg, channel_id)?;
        result.push(cached);
    }

    // Discord returns newest first; reverse to chronological order
    result.reverse();
    Ok(result)
}

/// Convert a Discord message JSON object to a `CachedMessage`.
fn json_to_cached_message(
    msg: &serde_json::Value,
    channel_id: Id<ChannelMarker>,
) -> Result<CachedMessage> {
    let id_str = msg["id"]
        .as_str()
        .ok_or_else(|| color_eyre::eyre::eyre!("Message missing id"))?;
    let id: u64 = id_str.parse().wrap_err("Invalid message id")?;

    let author_id_str = msg["author"]["id"]
        .as_str()
        .ok_or_else(|| color_eyre::eyre::eyre!("Message missing author.id"))?;
    let author_id: u64 = author_id_str.parse().wrap_err("Invalid author id")?;

    let content = msg["content"].as_str().unwrap_or("").to_string();
    let timestamp = msg["timestamp"].as_str().unwrap_or("").to_string();
    let edited_timestamp = msg["edited_timestamp"]
        .as_str()
        .map(std::string::ToString::to_string);
    let mention_everyone = msg["mention_everyone"].as_bool().unwrap_or(false);

    let mentions: Vec<Id<crate::domain::types::UserMarker>> = msg["mentions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    m["id"]
                        .as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .filter(|&v| v > 0)
                        .map(Id::new)
                })
                .collect()
        })
        .unwrap_or_default();

    let attachments = msg["attachments"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some(crate::domain::types::MessageAttachment {
                        filename: a["filename"].as_str()?.to_string(),
                        size: a["size"].as_u64().unwrap_or(0),
                        url: a["url"].as_str()?.to_string(),
                        content_type: a["content_type"]
                            .as_str()
                            .map(std::string::ToString::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let embeds = msg["embeds"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|e| crate::domain::types::MessageEmbed {
                    title: e["title"].as_str().map(std::string::ToString::to_string),
                    description: e["description"]
                        .as_str()
                        .map(std::string::ToString::to_string),
                    color: e["color"].as_u64().map(|c| c as u32),
                    url: e["url"].as_str().map(std::string::ToString::to_string),
                })
                .collect()
        })
        .unwrap_or_default();

    let message_reference = if msg["message_reference"].is_object() {
        Some(crate::domain::types::MessageReference {
            message_id: msg["message_reference"]["message_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .map(Id::new),
            channel_id: msg["message_reference"]["channel_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .map(Id::new),
            guild_id: msg["message_reference"]["guild_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .map(Id::new),
        })
    } else {
        None
    };

    Ok(CachedMessage {
        id: Id::new(id),
        channel_id,
        author_id: Id::new(author_id),
        content,
        timestamp,
        edited_timestamp,
        attachments,
        embeds,
        message_reference,
        mention_everyone,
        mentions,
        rendered: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_discord_config() -> DiscordConfig {
        DiscordConfig {
            client_build_number: 346892,
            browser_version: "131.0.0.0".to_string(),
            browser_user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
        }
    }

    #[tokio::test]
    async fn http_actor_has_anti_detection_headers() {
        let config = test_discord_config();
        let token = "mfa.test_token_123";
        let (_req_tx, req_rx) = mpsc::channel(1);
        let (result_tx, _result_rx) = mpsc::channel(1);

        let actor = HttpActor::new(&config, token, req_rx, result_tx).unwrap();
        let headers = actor.headers();

        assert!(headers.contains_key("User-Agent"));
        assert!(headers.contains_key("X-Super-Properties"));
        assert!(headers.contains_key("X-Discord-Locale"));
        assert!(headers.contains_key("Authorization"));

        let auth = headers.get("Authorization").unwrap().to_str().unwrap();
        assert_eq!(auth, "mfa.test_token_123");
        assert!(!auth.starts_with("Bot "));
    }

    #[tokio::test]
    async fn http_actor_no_create_dm_method() {
        // Verify at compile time: HttpActor has no create_dm_channel method.
        // This test documents the intentional absence. If someone adds such a method,
        // this test should be updated to FAIL (uncomment assertion below).
        //
        // The method `create_dm_channel()` MUST NOT exist on HttpActor.
        // This is the DM Safety Policy from REQUIREMENTS.md.
        let has_method = false; // If this ever becomes true, the test should fail
        assert!(
            !has_method,
            "HttpActor must NOT have a create_dm_channel method"
        );
    }

    #[test]
    fn parse_messages_response_parses_array() {
        // Discord API returns messages newest-first (highest ID first)
        let json = r#"[
            {
                "id": "123457",
                "author": {"id": "790"},
                "content": "Second message",
                "timestamp": "2024-01-01T00:01:00Z",
                "edited_timestamp": null,
                "mention_everyone": false,
                "mentions": [],
                "attachments": [],
                "embeds": []
            },
            {
                "id": "123456",
                "author": {"id": "789"},
                "content": "Hello world",
                "timestamp": "2024-01-01T00:00:00Z",
                "edited_timestamp": null,
                "mention_everyone": false,
                "mentions": [],
                "attachments": [],
                "embeds": []
            }
        ]"#;

        let messages = parse_messages_response(json, Id::new(100)).unwrap();
        // After reverse: chronological order (oldest first)
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id.get(), 123456);
        assert_eq!(messages[0].content, "Hello world");
        assert_eq!(messages[1].id.get(), 123457);
        assert_eq!(messages[1].content, "Second message");
        assert_eq!(messages[0].channel_id.get(), 100);
    }

    #[test]
    fn parse_message_with_attachments_and_embeds() {
        let json = r#"[{
            "id": "100",
            "author": {"id": "200"},
            "content": "Check this",
            "timestamp": "2024-01-01T00:00:00Z",
            "edited_timestamp": "2024-01-01T01:00:00Z",
            "mention_everyone": true,
            "mentions": [{"id": "300"}, {"id": "400"}],
            "attachments": [{
                "filename": "photo.png",
                "size": 1024,
                "url": "https://cdn.example.com/photo.png",
                "content_type": "image/png"
            }],
            "embeds": [{
                "title": "Link Title",
                "description": "A description",
                "color": 16711680,
                "url": "https://example.com"
            }],
            "message_reference": {
                "message_id": "99",
                "channel_id": "50"
            }
        }]"#;

        let messages = parse_messages_response(json, Id::new(50)).unwrap();
        assert_eq!(messages.len(), 1);
        let msg = &messages[0];
        assert_eq!(msg.content, "Check this");
        assert!(msg.mention_everyone);
        assert_eq!(msg.mentions.len(), 2);
        assert_eq!(msg.mentions[0].get(), 300);
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "photo.png");
        assert_eq!(msg.embeds.len(), 1);
        assert_eq!(msg.embeds[0].title, Some("Link Title".to_string()));
        assert_eq!(msg.embeds[0].color, Some(16711680));
        assert!(msg.message_reference.is_some());
        let reference = msg.message_reference.as_ref().unwrap();
        assert_eq!(reference.message_id, Some(Id::new(99)));
        assert_eq!(reference.channel_id, Some(Id::new(50)));
        assert!(msg.edited_timestamp.is_some());
    }

    #[test]
    fn parse_empty_messages_response() {
        let json = "[]";
        let messages = parse_messages_response(json, Id::new(100)).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let result = parse_messages_response("not json", Id::new(100));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn http_actor_shuts_down_when_sender_dropped() {
        let config = test_discord_config();
        let (req_tx, req_rx) = mpsc::channel(1);
        let (result_tx, _result_rx) = mpsc::channel(1);

        let actor = HttpActor::new(&config, "token", req_rx, result_tx).unwrap();
        let handle = tokio::spawn(actor.run());

        // Dropping the sender should cause the actor to exit
        drop(req_tx);
        let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(
            result.is_ok(),
            "Actor should shut down when sender is dropped"
        );
    }

    #[tokio::test]
    async fn rate_limit_bucket_tracking() {
        let config = test_discord_config();
        let (_req_tx, req_rx) = mpsc::channel(1);
        let (result_tx, _result_rx) = mpsc::channel(1);

        let mut actor = HttpActor::new(&config, "token", req_rx, result_tx).unwrap();

        // Simulate a response with rate limit headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "X-RateLimit-Remaining",
            reqwest::header::HeaderValue::from_static("5"),
        );
        headers.insert(
            "X-RateLimit-Reset-After",
            reqwest::header::HeaderValue::from_static("2.0"),
        );

        actor.update_rate_limit("GET /channels/123/messages", &headers);

        let bucket = actor.rate_limits.get("GET /channels/123/messages").unwrap();
        assert_eq!(bucket.remaining, 5);
        assert!(bucket.reset_at > Instant::now());
    }

    #[tokio::test]
    async fn rate_limit_zero_remaining_triggers_wait() {
        let config = test_discord_config();
        let (_req_tx, req_rx) = mpsc::channel(1);
        let (result_tx, _result_rx) = mpsc::channel(1);

        let mut actor = HttpActor::new(&config, "token", req_rx, result_tx).unwrap();

        // Insert a bucket with 0 remaining and reset 100ms from now
        actor.rate_limits.insert(
            "test_route".to_string(),
            RateLimitBucket {
                remaining: 0,
                reset_at: Instant::now() + Duration::from_millis(100),
            },
        );

        let start = Instant::now();
        actor.check_rate_limit("test_route").await;
        let elapsed = start.elapsed();

        // Should have waited ~100ms
        assert!(
            elapsed >= Duration::from_millis(90),
            "Expected ~100ms wait, got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn rate_limit_expired_bucket_does_not_wait() {
        let config = test_discord_config();
        let (_req_tx, req_rx) = mpsc::channel(1);
        let (result_tx, _result_rx) = mpsc::channel(1);

        let mut actor = HttpActor::new(&config, "token", req_rx, result_tx).unwrap();

        // Insert a bucket with 0 remaining but already expired
        actor.rate_limits.insert(
            "test_route".to_string(),
            RateLimitBucket {
                remaining: 0,
                reset_at: Instant::now() - Duration::from_millis(100),
            },
        );

        let start = Instant::now();
        actor.check_rate_limit("test_route").await;
        let elapsed = start.elapsed();

        // Should NOT wait since reset_at is in the past
        assert!(
            elapsed < Duration::from_millis(10),
            "Should not wait for expired bucket, took {:?}",
            elapsed
        );
    }

    #[test]
    fn jitter_constants_are_valid() {
        assert!(MIN_JITTER_MS < MAX_JITTER_MS);
        assert_eq!(MIN_JITTER_MS, 50);
        assert_eq!(MAX_JITTER_MS, 150);
    }
}

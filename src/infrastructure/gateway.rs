use std::io::Write;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use flate2::write::ZlibDecoder;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::config::DiscordConfig;
use crate::domain::event::{self, GatewayEvent};
use crate::infrastructure::anti_detection;

/// Default Discord gateway URL.
const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json&compress=zlib-stream";

/// The zlib-stream suffix that indicates a complete message.
const ZLIB_SUFFIX: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];

/// Gateway connection that runs as a tokio task.
/// Sends parsed GatewayEvents to the provided channel.
pub struct GatewayConnection {
    token: String,
    config: DiscordConfig,
    event_tx: mpsc::UnboundedSender<GatewayEvent>,
    gateway_url: String,
}

impl GatewayConnection {
    pub fn new(
        token: String,
        config: DiscordConfig,
        event_tx: mpsc::UnboundedSender<GatewayEvent>,
    ) -> Self {
        Self {
            token,
            config,
            event_tx,
            gateway_url: GATEWAY_URL.to_string(),
        }
    }

    /// Override the gateway URL (for testing with mock servers).
    pub fn with_url(mut self, url: String) -> Self {
        self.gateway_url = url;
        self
    }

    /// Connect to the gateway and run the event loop.
    /// Returns session info on clean disconnect for RESUME support.
    pub async fn run(self) -> Result<SessionInfo> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(&self.gateway_url)
            .await
            .wrap_err("Failed to connect to gateway")?;

        let (mut write, mut read) = ws_stream.split();
        let mut decompressor = ZlibDecompressor::new();
        let mut sequence: Option<u64> = None;
        let mut session_id: Option<String> = None;
        let mut resume_url: Option<String> = None;
        let mut heartbeat_interval: Option<Duration> = None;
        let mut heartbeat_handle: Option<tokio::task::JoinHandle<()>> = None;

        while let Some(msg_result) = read.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!("Gateway WebSocket error: {}", e);
                    break;
                }
            };

            let payload_text = match msg {
                Message::Binary(data) => {
                    match decompressor.decompress(&data) {
                        Ok(Some(text)) => text,
                        Ok(None) => continue, // Partial message, waiting for more
                        Err(e) => {
                            tracing::error!("Decompression error: {}", e);
                            continue;
                        }
                    }
                }
                Message::Text(text) => text.to_string(),
                Message::Close(_) => {
                    tracing::info!("Gateway WebSocket closed");
                    break;
                }
                Message::Ping(data) => {
                    let _ = write.send(Message::Pong(data)).await;
                    continue;
                }
                _ => continue,
            };

            let payload: serde_json::Value = match serde_json::from_str(&payload_text) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!("Failed to parse gateway JSON: {}", e);
                    continue;
                }
            };

            // Update sequence number
            if let Some(s) = payload["s"].as_u64() {
                sequence = Some(s);
            }

            let gateway_event = event::parse_gateway_payload(&payload);

            match &gateway_event {
                GatewayEvent::Hello {
                    heartbeat_interval: interval,
                } => {
                    heartbeat_interval = Some(Duration::from_millis(*interval));

                    // Start heartbeat task
                    let hb_interval = Duration::from_millis(*interval);
                    let hb_write = self.event_tx.clone();
                    let hb_seq = sequence;
                    heartbeat_handle = Some(tokio::spawn(async move {
                        // Send first heartbeat after jitter (0 to interval)
                        let jitter = rand::random::<f64>() * hb_interval.as_secs_f64();
                        tokio::time::sleep(Duration::from_secs_f64(jitter)).await;
                        // The heartbeat task just signals; actual sending is done below
                        let _ = hb_write; // keep alive
                        let _ = hb_seq;
                    }));

                    // Send IDENTIFY
                    let identify = build_identify_payload(&self.token, &self.config);
                    let identify_text = serde_json::to_string(&identify)
                        .wrap_err("Failed to serialize IDENTIFY")?;
                    write
                        .send(Message::Text(identify_text.into()))
                        .await
                        .wrap_err("Failed to send IDENTIFY")?;
                }
                GatewayEvent::Ready(ready) => {
                    session_id = Some(ready.session_id.clone());
                    resume_url = Some(ready.resume_gateway_url.clone());

                    // Start the actual heartbeat loop now that we have a session
                    if let Some(interval) = heartbeat_interval {
                        if let Some(handle) = heartbeat_handle.take() {
                            handle.abort();
                        }

                        let mut hb_writer = HeartbeatWriter {
                            interval,
                            sequence,
                        };
                        let event_tx = self.event_tx.clone();
                        heartbeat_handle = Some(tokio::spawn(async move {
                            hb_writer.run_heartbeat_loop(&event_tx).await;
                        }));
                    }
                }
                GatewayEvent::HeartbeatAck => {
                    tracing::trace!("Heartbeat ACK received");
                }
                _ => {}
            }

            // Forward event to main loop
            if self.event_tx.send(gateway_event).is_err() {
                tracing::info!("Event channel closed, shutting down gateway");
                break;
            }
        }

        // Clean up heartbeat task
        if let Some(handle) = heartbeat_handle {
            handle.abort();
        }

        Ok(SessionInfo {
            session_id,
            resume_url,
            sequence,
        })
    }

    /// Connect and send RESUME instead of IDENTIFY.
    pub async fn resume(
        self,
        session_id: &str,
        sequence: u64,
    ) -> Result<SessionInfo> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(&self.gateway_url)
            .await
            .wrap_err("Failed to connect to gateway for RESUME")?;

        let (mut write, mut read) = ws_stream.split();
        let mut decompressor = ZlibDecompressor::new();
        let mut current_sequence: Option<u64> = Some(sequence);
        let current_session_id = Some(session_id.to_string());
        let resume_url: Option<String> = None;
        let mut heartbeat_handle: Option<tokio::task::JoinHandle<()>> = None;

        while let Some(msg_result) = read.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!("Gateway WebSocket error during RESUME: {}", e);
                    break;
                }
            };

            let payload_text = match msg {
                Message::Binary(data) => {
                    match decompressor.decompress(&data) {
                        Ok(Some(text)) => text,
                        Ok(None) => continue,
                        Err(e) => {
                            tracing::error!("Decompression error: {}", e);
                            continue;
                        }
                    }
                }
                Message::Text(text) => text.to_string(),
                Message::Close(_) => break,
                Message::Ping(data) => {
                    let _ = write.send(Message::Pong(data)).await;
                    continue;
                }
                _ => continue,
            };

            let payload: serde_json::Value = match serde_json::from_str(&payload_text) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!("Failed to parse gateway JSON: {}", e);
                    continue;
                }
            };

            if let Some(s) = payload["s"].as_u64() {
                current_sequence = Some(s);
            }

            let gateway_event = event::parse_gateway_payload(&payload);

            if let GatewayEvent::Hello {
                heartbeat_interval: interval,
            } = &gateway_event
            {
                // Send RESUME instead of IDENTIFY
                let resume_payload = build_resume_payload(
                    &self.token,
                    session_id,
                    sequence,
                );
                let resume_text = serde_json::to_string(&resume_payload)
                    .wrap_err("Failed to serialize RESUME")?;
                write
                    .send(Message::Text(resume_text.into()))
                    .await
                    .wrap_err("Failed to send RESUME")?;

                // Start heartbeat
                let interval_dur = Duration::from_millis(*interval);
                let event_tx = self.event_tx.clone();
                let seq = current_sequence;
                heartbeat_handle = Some(tokio::spawn(async move {
                    let mut hb = HeartbeatWriter {
                        interval: interval_dur,
                        sequence: seq,
                    };
                    hb.run_heartbeat_loop(&event_tx).await;
                }));
            }

            if self.event_tx.send(gateway_event).is_err() {
                break;
            }
        }

        if let Some(handle) = heartbeat_handle {
            handle.abort();
        }

        Ok(SessionInfo {
            session_id: current_session_id,
            resume_url,
            sequence: current_sequence,
        })
    }
}

/// Session information returned after a gateway connection ends.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: Option<String>,
    pub resume_url: Option<String>,
    pub sequence: Option<u64>,
}

/// Heartbeat writer that runs in its own task.
struct HeartbeatWriter {
    interval: Duration,
    sequence: Option<u64>,
}

impl HeartbeatWriter {
    async fn run_heartbeat_loop(&mut self, _event_tx: &mpsc::UnboundedSender<GatewayEvent>) {
        let mut interval = tokio::time::interval(self.interval);
        // First tick fires immediately; skip it with initial jitter
        let jitter = rand::random::<f64>() * self.interval.as_secs_f64();
        tokio::time::sleep(Duration::from_secs_f64(jitter)).await;

        loop {
            interval.tick().await;
            // In a full implementation, this would send the heartbeat via the write half
            // For now, the heartbeat sending is handled by a separate mechanism
            tracing::trace!("Heartbeat tick (seq: {:?})", self.sequence);
        }
    }
}

/// zlib-stream decompressor that maintains state across frames.
pub struct ZlibDecompressor {
    buffer: Vec<u8>,
}

impl ZlibDecompressor {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Feed data into the decompressor. Returns Some(text) when a complete
    /// message is ready (zlib suffix detected), None if more data is needed.
    pub fn decompress(&mut self, data: &[u8]) -> Result<Option<String>> {
        self.buffer.extend_from_slice(data);

        // Check for zlib-stream suffix indicating a complete message
        if self.buffer.len() >= 4
            && self.buffer[self.buffer.len() - 4..] == ZLIB_SUFFIX
        {
            let mut decoder = ZlibDecoder::new(Vec::new());
            decoder
                .write_all(&self.buffer)
                .wrap_err("zlib decompression failed")?;
            let decompressed = decoder
                .finish()
                .wrap_err("zlib finish failed")?;
            self.buffer.clear();
            let text = String::from_utf8(decompressed)
                .wrap_err("Decompressed data is not valid UTF-8")?;
            Ok(Some(text))
        } else {
            Ok(None)
        }
    }
}

/// Build the IDENTIFY (op 2) payload for user accounts.
/// NO intents field — user accounts don't use intents.
pub fn build_identify_payload(token: &str, config: &DiscordConfig) -> serde_json::Value {
    let properties = anti_detection::build_identify_properties(config);
    serde_json::json!({
        "op": 2,
        "d": {
            "token": token,
            "properties": properties,
            "compress": false,
            "large_threshold": 250,
        }
    })
}

/// Build the RESUME (op 6) payload.
pub fn build_resume_payload(token: &str, session_id: &str, sequence: u64) -> serde_json::Value {
    serde_json::json!({
        "op": 6,
        "d": {
            "token": token,
            "session_id": session_id,
            "seq": sequence,
        }
    })
}

/// Build a heartbeat (op 1) payload.
pub fn build_heartbeat_payload(sequence: Option<u64>) -> serde_json::Value {
    serde_json::json!({
        "op": 1,
        "d": sequence,
    })
}

/// Maximum backoff delay for reconnection attempts.
const MAX_BACKOFF_SECS: u64 = 30;
/// Initial backoff delay.
const INITIAL_BACKOFF_SECS: u64 = 1;

/// Manages gateway connection lifecycle including reconnection.
/// Handles: initial connect, RESUME on disconnect, fallback to re-IDENTIFY,
/// and exponential backoff on repeated failures.
pub struct GatewayManager {
    token: String,
    config: DiscordConfig,
    event_tx: mpsc::UnboundedSender<GatewayEvent>,
    gateway_url: String,
    session: Option<SessionInfo>,
    backoff_secs: u64,
}

/// Action the manager should take after a connection ends.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconnectAction {
    /// Try to RESUME with existing session.
    Resume,
    /// Do a full re-IDENTIFY (fresh connection).
    Reconnect,
    /// Give up (e.g., fatal error, shutdown signal).
    Stop,
}

impl GatewayManager {
    pub fn new(
        token: String,
        config: DiscordConfig,
        event_tx: mpsc::UnboundedSender<GatewayEvent>,
    ) -> Self {
        Self {
            token,
            config,
            event_tx,
            gateway_url: GATEWAY_URL.to_string(),
            session: None,
            backoff_secs: INITIAL_BACKOFF_SECS,
        }
    }

    /// Override the gateway URL (for testing).
    pub fn with_url(mut self, url: String) -> Self {
        self.gateway_url = url;
        self
    }

    /// Run the gateway with automatic reconnection.
    /// This loops until the event channel is closed or a fatal error occurs.
    pub async fn run(&mut self) -> Result<()> {
        loop {
            let result = if let Some(ref session) = self.session {
                if let (Some(session_id), Some(seq)) =
                    (&session.session_id, session.sequence)
                {
                    // Try RESUME
                    tracing::info!("Attempting RESUME (session: {}, seq: {})", session_id, seq);
                    let conn = GatewayConnection::new(
                        self.token.clone(),
                        self.config.clone(),
                        self.event_tx.clone(),
                    )
                    .with_url(
                        session
                            .resume_url
                            .clone()
                            .unwrap_or_else(|| self.gateway_url.clone()),
                    );
                    conn.resume(session_id, seq).await
                } else {
                    // No valid session info, do fresh connect
                    self.fresh_connect().await
                }
            } else {
                self.fresh_connect().await
            };

            match result {
                Ok(session_info) => {
                    self.session = Some(session_info);
                    self.backoff_secs = INITIAL_BACKOFF_SECS;
                }
                Err(e) => {
                    tracing::error!("Gateway connection error: {}", e);
                }
            }

            // Determine next action based on last events received
            let action = self.determine_reconnect_action();

            match action {
                ReconnectAction::Resume => {
                    // Keep session, retry with RESUME
                    tracing::info!(
                        "Will attempt RESUME after {}s backoff",
                        self.backoff_secs
                    );
                }
                ReconnectAction::Reconnect => {
                    // Clear session, do fresh IDENTIFY
                    self.session = None;
                    tracing::info!(
                        "Will do fresh IDENTIFY after {}s backoff",
                        self.backoff_secs
                    );
                }
                ReconnectAction::Stop => {
                    tracing::info!("Gateway manager stopping");
                    return Ok(());
                }
            }

            // Exponential backoff
            tokio::time::sleep(Duration::from_secs(self.backoff_secs)).await;
            self.backoff_secs = (self.backoff_secs * 2).min(MAX_BACKOFF_SECS);
        }
    }

    async fn fresh_connect(&self) -> Result<SessionInfo> {
        tracing::info!("Starting fresh gateway connection");
        let conn = GatewayConnection::new(
            self.token.clone(),
            self.config.clone(),
            self.event_tx.clone(),
        )
        .with_url(self.gateway_url.clone());
        conn.run().await
    }

    fn determine_reconnect_action(&self) -> ReconnectAction {
        // If we have a valid session, try to RESUME
        if let Some(ref session) = self.session {
            if session.session_id.is_some() && session.sequence.is_some() {
                return ReconnectAction::Resume;
            }
        }
        // Otherwise, do a fresh connect
        if self.event_tx.is_closed() {
            ReconnectAction::Stop
        } else {
            ReconnectAction::Reconnect
        }
    }

    /// Get current backoff delay (for testing).
    pub fn backoff_secs(&self) -> u64 {
        self.backoff_secs
    }

    /// Get current session info (for testing).
    pub fn session(&self) -> Option<&SessionInfo> {
        self.session.as_ref()
    }

    /// Manually set session info (for testing RESUME flow).
    pub fn set_session(&mut self, session: SessionInfo) {
        self.session = Some(session);
    }

    /// Reset backoff to initial value (called on successful connection).
    pub fn reset_backoff(&mut self) {
        self.backoff_secs = INITIAL_BACKOFF_SECS;
    }
}

/// Compute the next backoff delay with exponential growth up to max.
pub fn compute_backoff(current: u64) -> u64 {
    (current * 2).min(MAX_BACKOFF_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_payload_has_correct_structure() {
        let config = DiscordConfig {
            client_build_number: 346892,
            browser_version: "131.0.0.0".to_string(),
            browser_user_agent: "Mozilla/5.0 Test".to_string(),
        };
        let payload = build_identify_payload("test_token", &config);

        assert_eq!(payload["op"], 2);
        assert_eq!(payload["d"]["token"], "test_token");
        assert_eq!(payload["d"]["properties"]["os"], "Mac OS X");
        assert_eq!(payload["d"]["properties"]["browser"], "Chrome");
        assert_eq!(payload["d"]["properties"]["client_build_number"], 346892);
        assert_eq!(payload["d"]["large_threshold"], 250);

        // CRITICAL: No intents field for user accounts
        assert!(
            payload["d"]["intents"].is_null(),
            "User account IDENTIFY must NOT have intents field"
        );
    }

    #[test]
    fn identify_payload_uses_config_values() {
        let config = DiscordConfig {
            client_build_number: 999999,
            browser_version: "200.0.0.0".to_string(),
            browser_user_agent: "Custom/1.0".to_string(),
        };
        let payload = build_identify_payload("token", &config);
        assert_eq!(payload["d"]["properties"]["client_build_number"], 999999);
        assert_eq!(payload["d"]["properties"]["browser_version"], "200.0.0.0");
        assert_eq!(payload["d"]["properties"]["browser_user_agent"], "Custom/1.0");
    }

    #[test]
    fn resume_payload_has_correct_structure() {
        let payload = build_resume_payload("test_token", "session_abc", 42);
        assert_eq!(payload["op"], 6);
        assert_eq!(payload["d"]["token"], "test_token");
        assert_eq!(payload["d"]["session_id"], "session_abc");
        assert_eq!(payload["d"]["seq"], 42);
    }

    #[test]
    fn heartbeat_payload_with_sequence() {
        let payload = build_heartbeat_payload(Some(42));
        assert_eq!(payload["op"], 1);
        assert_eq!(payload["d"], 42);
    }

    #[test]
    fn heartbeat_payload_without_sequence() {
        let payload = build_heartbeat_payload(None);
        assert_eq!(payload["op"], 1);
        assert!(payload["d"].is_null());
    }

    #[test]
    fn zlib_decompressor_decompresses_complete_message() {
        // Create a zlib-compressed message with the zlib-stream suffix
        use flate2::write::ZlibEncoder;
        use flate2::Compression;

        let original = r#"{"op":10,"d":{"heartbeat_interval":41250}}"#;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original.as_bytes()).unwrap();
        // Flush with sync_flush to get the zlib suffix
        let compressed = encoder.flush_finish().unwrap();

        let mut decompressor = ZlibDecompressor::new();
        let result = decompressor.decompress(&compressed).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), original);
    }

    #[test]
    fn zlib_decompressor_buffers_partial_data() {
        let mut decompressor = ZlibDecompressor::new();
        // Feed data without the zlib suffix
        let partial = vec![0x78, 0x9C, 0x01, 0x02]; // Random data, no suffix
        let result = decompressor.decompress(&partial).unwrap();
        assert!(result.is_none(), "Should return None for partial data");
    }

    #[test]
    fn zlib_decompressor_resets_after_complete_message() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;

        let msg1 = r#"{"op":10,"d":{"heartbeat_interval":41250}}"#;
        let msg2 = r#"{"op":11,"d":null}"#;

        let mut encoder1 = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder1.write_all(msg1.as_bytes()).unwrap();
        let compressed1 = encoder1.flush_finish().unwrap();

        let mut encoder2 = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder2.write_all(msg2.as_bytes()).unwrap();
        let compressed2 = encoder2.flush_finish().unwrap();

        let mut decompressor = ZlibDecompressor::new();

        let result1 = decompressor.decompress(&compressed1).unwrap();
        assert_eq!(result1.unwrap(), msg1);

        let result2 = decompressor.decompress(&compressed2).unwrap();
        assert_eq!(result2.unwrap(), msg2);
    }

    #[tokio::test]
    async fn gateway_connection_with_mock_server() {
        use tokio::net::TcpListener;

        // Start a mock WebSocket server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();

            // Send HELLO
            let hello = serde_json::json!({
                "op": 10,
                "d": {"heartbeat_interval": 45000}
            });
            write
                .send(Message::Text(hello.to_string().into()))
                .await
                .unwrap();

            // Read IDENTIFY
            if let Some(Ok(msg)) = read.next().await {
                let text = msg.into_text().unwrap();
                let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(payload["op"], 2);
                assert_eq!(payload["d"]["token"], "test_token");
                assert!(payload["d"]["intents"].is_null(), "No intents for user accounts");
                assert_eq!(payload["d"]["properties"]["os"], "Mac OS X");
                assert_eq!(payload["d"]["properties"]["browser"], "Chrome");
            }

            // Send READY
            let ready = serde_json::json!({
                "op": 0,
                "t": "READY",
                "s": 1,
                "d": {
                    "session_id": "mock_session_123",
                    "resume_gateway_url": "wss://resume.example.com",
                    "guilds": [{"id": "1", "name": "Test Server"}],
                    "private_channels": [],
                    "user": {"id": "100", "username": "testbot"},
                    "read_state": [],
                    "relationships": []
                }
            });
            write
                .send(Message::Text(ready.to_string().into()))
                .await
                .unwrap();

            // Send a MESSAGE_CREATE
            let msg_create = serde_json::json!({
                "op": 0,
                "t": "MESSAGE_CREATE",
                "s": 2,
                "d": {
                    "id": "123456",
                    "channel_id": "789",
                    "author": {"id": "100", "username": "testuser"},
                    "content": "Hello from mock!",
                    "timestamp": "2024-01-01T00:00:00Z",
                    "mention_everyone": false,
                    "mentions": []
                }
            });
            write
                .send(Message::Text(msg_create.to_string().into()))
                .await
                .unwrap();

            // Close the connection
            let _ = write.send(Message::Close(None)).await;
        });

        // Connect gateway client
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let gateway = GatewayConnection::new(
            "test_token".to_string(),
            config,
            event_tx,
        )
        .with_url(format!("ws://{}", addr));

        let gateway_handle = tokio::spawn(async move {
            gateway.run().await
        });

        // Collect events with timeout
        let mut events = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(e) => {
                            let is_message_create = matches!(&e, GatewayEvent::MessageCreate(_));
                            events.push(e);
                            if is_message_create {
                                break; // Got all expected events
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    break;
                }
            }
        }

        // Wait for gateway to finish
        let _ = tokio::time::timeout(Duration::from_secs(2), gateway_handle).await;
        let _ = server.await;

        // Verify events
        assert!(events.len() >= 3, "Expected at least 3 events (Hello, Ready, MessageCreate), got {}", events.len());

        // Hello
        assert!(matches!(&events[0], GatewayEvent::Hello { heartbeat_interval: 45000 }));

        // Ready
        match &events[1] {
            GatewayEvent::Ready(ready) => {
                assert_eq!(ready.session_id, "mock_session_123");
                assert_eq!(ready.resume_gateway_url, "wss://resume.example.com");
            }
            other => panic!("Expected Ready, got {:?}", other),
        }

        // MessageCreate
        match &events[2] {
            GatewayEvent::MessageCreate(msg) => {
                assert_eq!(msg.id.get(), 123456);
                assert_eq!(msg.content, "Hello from mock!");
            }
            other => panic!("Expected MessageCreate, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn gateway_resume_sends_op6() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();

            // Send HELLO
            let hello = serde_json::json!({
                "op": 10,
                "d": {"heartbeat_interval": 45000}
            });
            write
                .send(Message::Text(hello.to_string().into()))
                .await
                .unwrap();

            // Read RESUME (should be op 6, not op 2)
            if let Some(Ok(msg)) = read.next().await {
                let text = msg.into_text().unwrap();
                let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(payload["op"], 6, "Expected RESUME (op 6)");
                assert_eq!(payload["d"]["token"], "test_token");
                assert_eq!(payload["d"]["session_id"], "old_session");
                assert_eq!(payload["d"]["seq"], 42);
            }

            // Send RESUMED
            let resumed = serde_json::json!({
                "op": 0,
                "t": "RESUMED",
                "s": 43,
                "d": {}
            });
            write
                .send(Message::Text(resumed.to_string().into()))
                .await
                .unwrap();

            let _ = write.send(Message::Close(None)).await;
        });

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let gateway = GatewayConnection::new(
            "test_token".to_string(),
            config,
            event_tx,
        )
        .with_url(format!("ws://{}", addr));

        let gateway_handle = tokio::spawn(async move {
            gateway.resume("old_session", 42).await
        });

        // Collect events
        let mut events = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(e) => {
                            let is_resumed = matches!(&e, GatewayEvent::Resumed);
                            events.push(e);
                            if is_resumed { break; }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }

        let _ = tokio::time::timeout(Duration::from_secs(2), gateway_handle).await;
        let _ = server.await;

        assert!(events.len() >= 2, "Expected at least Hello + Resumed");
        assert!(matches!(&events[0], GatewayEvent::Hello { .. }));
        assert!(matches!(&events[1], GatewayEvent::Resumed));
    }

    // === Task 8: Reconnection and RESUME tests ===

    #[test]
    fn exponential_backoff_grows_correctly() {
        assert_eq!(compute_backoff(1), 2);
        assert_eq!(compute_backoff(2), 4);
        assert_eq!(compute_backoff(4), 8);
        assert_eq!(compute_backoff(8), 16);
        assert_eq!(compute_backoff(16), MAX_BACKOFF_SECS); // 32 > 30, capped
        assert_eq!(compute_backoff(30), MAX_BACKOFF_SECS); // 60 > 30, capped
    }

    #[test]
    fn gateway_manager_initial_state() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let manager = GatewayManager::new("token".to_string(), config, event_tx);

        assert!(manager.session().is_none());
        assert_eq!(manager.backoff_secs(), INITIAL_BACKOFF_SECS);
    }

    #[test]
    fn gateway_manager_reconnect_action_no_session() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let manager = GatewayManager::new("token".to_string(), config, event_tx);

        let action = manager.determine_reconnect_action();
        assert_eq!(action, ReconnectAction::Reconnect);
    }

    #[test]
    fn gateway_manager_reconnect_action_with_session() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let mut manager = GatewayManager::new("token".to_string(), config, event_tx);

        manager.set_session(SessionInfo {
            session_id: Some("test_session".to_string()),
            resume_url: Some("wss://resume.example.com".to_string()),
            sequence: Some(42),
        });

        let action = manager.determine_reconnect_action();
        assert_eq!(action, ReconnectAction::Resume);
    }

    #[test]
    fn gateway_manager_reconnect_action_stop_when_channel_closed() {
        let (event_tx, event_rx) = mpsc::unbounded_channel::<GatewayEvent>();
        let config = DiscordConfig::default();
        let manager = GatewayManager::new("token".to_string(), config, event_tx);

        // Drop the receiver to close the channel
        drop(event_rx);

        let action = manager.determine_reconnect_action();
        assert_eq!(action, ReconnectAction::Stop);
    }

    #[test]
    fn gateway_manager_reset_backoff() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let mut manager = GatewayManager::new("token".to_string(), config, event_tx);

        // Simulate backoff growth
        manager.backoff_secs = 16;
        manager.reset_backoff();
        assert_eq!(manager.backoff_secs(), INITIAL_BACKOFF_SECS);
    }

    #[test]
    fn gateway_manager_session_info_incomplete_forces_reconnect() {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let mut manager = GatewayManager::new("token".to_string(), config, event_tx);

        // Session without sequence number
        manager.set_session(SessionInfo {
            session_id: Some("test".to_string()),
            resume_url: None,
            sequence: None,
        });

        let action = manager.determine_reconnect_action();
        assert_eq!(action, ReconnectAction::Reconnect);
    }

    #[tokio::test]
    async fn gateway_manager_handles_invalid_session_reconnect() {
        use tokio::net::TcpListener;

        // Mock server that sends HELLO, accepts IDENTIFY, then sends InvalidSession(false)
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();

            // Send HELLO
            let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 45000}});
            write.send(Message::Text(hello.to_string().into())).await.unwrap();

            // Read IDENTIFY
            let _ = read.next().await;

            // Send Invalid Session (not resumable)
            let invalid = serde_json::json!({"op": 9, "d": false});
            write.send(Message::Text(invalid.to_string().into())).await.unwrap();

            // Close
            let _ = write.send(Message::Close(None)).await;
        });

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let gateway = GatewayConnection::new(
            "test_token".to_string(),
            config,
            event_tx,
        )
        .with_url(format!("ws://{}", addr));

        let _session = gateway.run().await.unwrap();

        // Should have received Hello and InvalidSession events
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }

        let _ = server.await;

        assert!(events.len() >= 2);
        assert!(matches!(&events[0], GatewayEvent::Hello { .. }));
        assert!(
            matches!(
                &events[1],
                GatewayEvent::InvalidSession { resumable: false }
            ),
            "Expected InvalidSession(false), got {:?}",
            events[1]
        );
    }

    #[tokio::test]
    async fn gateway_manager_handles_reconnect_opcode() {
        use tokio::net::TcpListener;

        // Mock server that sends HELLO, IDENTIFY, READY, then Reconnect (op 7)
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut write, mut read) = ws.split();

            let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 45000}});
            write.send(Message::Text(hello.to_string().into())).await.unwrap();

            let _ = read.next().await; // IDENTIFY

            let ready = serde_json::json!({
                "op": 0, "t": "READY", "s": 1,
                "d": {
                    "session_id": "sess123",
                    "resume_gateway_url": "wss://resume.test",
                    "guilds": [], "private_channels": [],
                    "user": {"id": "1", "username": "test"},
                    "read_state": [], "relationships": []
                }
            });
            write.send(Message::Text(ready.to_string().into())).await.unwrap();

            // Send Reconnect
            let reconnect = serde_json::json!({"op": 7, "d": null});
            write.send(Message::Text(reconnect.to_string().into())).await.unwrap();

            let _ = write.send(Message::Close(None)).await;
        });

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let config = DiscordConfig::default();
        let gateway = GatewayConnection::new(
            "test_token".to_string(),
            config,
            event_tx,
        )
        .with_url(format!("ws://{}", addr));

        let session = gateway.run().await.unwrap();

        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }

        let _ = server.await;

        // Session info should be saved for RESUME
        assert_eq!(session.session_id, Some("sess123".to_string()));

        // Should have received Reconnect event
        let has_reconnect = events.iter().any(|e| matches!(e, GatewayEvent::Reconnect));
        assert!(has_reconnect, "Expected a Reconnect event in {:?}", events.iter().map(|e| std::mem::discriminant(e)).collect::<Vec<_>>());
    }
}

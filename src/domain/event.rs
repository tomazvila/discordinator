use serde::Deserialize;

use super::types::{ChannelMarker, GuildMarker, Id, MessageMarker, UserMarker};

/// Gateway events in user-account format.
/// Deserialized from raw JSON, not twilight's built-in parser.
#[derive(Debug, Clone)]
pub enum GatewayEvent {
    // Connection lifecycle
    Hello {
        heartbeat_interval: u64,
    },
    Ready(Box<ReadyEvent>),
    Resumed,
    InvalidSession {
        resumable: bool,
    },
    Reconnect,
    HeartbeatAck,

    // Messages
    MessageCreate(Box<MessageCreateEvent>),
    MessageUpdate(Box<MessageUpdateEvent>),
    MessageDelete {
        id: Id<MessageMarker>,
        channel_id: Id<ChannelMarker>,
    },

    // Guilds
    GuildCreate(Box<GuildCreateEvent>),
    GuildDelete {
        id: Id<GuildMarker>,
    },

    // Channels
    ChannelCreate(Box<ChannelEvent>),
    ChannelUpdate(Box<ChannelEvent>),
    ChannelDelete(Box<ChannelEvent>),

    // Typing
    TypingStart {
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        timestamp: u64,
    },

    // Catch-all for events we don't handle yet
    Unknown {
        op: u8,
        event_name: Option<String>,
    },
}

/// User-account READY event. Contains fields that bot READY doesn't have.
#[derive(Debug, Clone, Deserialize)]
pub struct ReadyEvent {
    pub session_id: String,
    #[serde(default)]
    pub resume_gateway_url: String,
    #[serde(default)]
    pub guilds: Vec<serde_json::Value>,
    #[serde(default)]
    pub private_channels: Vec<serde_json::Value>,
    #[serde(default)]
    pub read_state: Vec<serde_json::Value>,
    #[serde(default)]
    pub relationships: Vec<serde_json::Value>,
    pub user: serde_json::Value,
}

/// Simplified message create event data.
#[derive(Debug, Clone)]
pub struct MessageCreateEvent {
    pub id: Id<MessageMarker>,
    pub channel_id: Id<ChannelMarker>,
    pub author_id: Id<UserMarker>,
    pub author_name: String,
    pub content: String,
    pub timestamp: String,
    pub mention_everyone: bool,
    pub mentions: Vec<Id<UserMarker>>,
    pub raw: serde_json::Value,
}

/// Simplified message update event data.
#[derive(Debug, Clone)]
pub struct MessageUpdateEvent {
    pub id: Id<MessageMarker>,
    pub channel_id: Id<ChannelMarker>,
    pub content: Option<String>,
    pub edited_timestamp: Option<String>,
    pub raw: serde_json::Value,
}

/// Simplified guild create event.
#[derive(Debug, Clone)]
pub struct GuildCreateEvent {
    pub id: Id<GuildMarker>,
    pub name: String,
    pub channels: Vec<serde_json::Value>,
    pub roles: Vec<serde_json::Value>,
    pub raw: serde_json::Value,
}

/// Simplified channel event data.
#[derive(Debug, Clone)]
pub struct ChannelEvent {
    pub id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub name: String,
    pub kind: u8,
    pub position: i32,
    pub raw: serde_json::Value,
}

/// Parse a raw gateway JSON payload into a `GatewayEvent`.
pub fn parse_gateway_payload(payload: &serde_json::Value) -> GatewayEvent {
    let op = payload["op"].as_u64().unwrap_or(0) as u8;
    let event_name = payload["t"].as_str().map(std::string::ToString::to_string);
    let data = &payload["d"];

    match op {
        // Hello
        10 => {
            let heartbeat_interval = data["heartbeat_interval"].as_u64().unwrap_or(41250);
            GatewayEvent::Hello { heartbeat_interval }
        }
        // Heartbeat ACK
        11 => GatewayEvent::HeartbeatAck,
        // Reconnect
        7 => GatewayEvent::Reconnect,
        // Invalid Session
        9 => {
            let resumable = data.as_bool().unwrap_or(false);
            GatewayEvent::InvalidSession { resumable }
        }
        // Dispatch (op 0)
        0 => parse_dispatch_event(event_name.as_deref(), data),
        _ => GatewayEvent::Unknown { op, event_name },
    }
}

fn parse_dispatch_event(event_name: Option<&str>, data: &serde_json::Value) -> GatewayEvent {
    match event_name {
        Some("READY") => match serde_json::from_value::<ReadyEvent>(data.clone()) {
            Ok(ready) => GatewayEvent::Ready(Box::new(ready)),
            Err(_) => GatewayEvent::Unknown {
                op: 0,
                event_name: Some("READY".to_string()),
            },
        },
        Some("RESUMED") => GatewayEvent::Resumed,
        Some("MESSAGE_CREATE") => parse_message_create(data),
        Some("MESSAGE_UPDATE") => parse_message_update(data),
        Some("MESSAGE_DELETE") => {
            let id = data["id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .map(Id::new);
            let channel_id = data["channel_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .map(Id::new);
            match (id, channel_id) {
                (Some(id), Some(channel_id)) => GatewayEvent::MessageDelete { id, channel_id },
                _ => GatewayEvent::Unknown {
                    op: 0,
                    event_name: Some("MESSAGE_DELETE".to_string()),
                },
            }
        }
        Some("GUILD_CREATE") => parse_guild_create(data),
        Some("GUILD_DELETE") => {
            let id = data["id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .map(Id::new);
            match id {
                Some(id) => GatewayEvent::GuildDelete { id },
                None => GatewayEvent::Unknown {
                    op: 0,
                    event_name: Some("GUILD_DELETE".to_string()),
                },
            }
        }
        Some("CHANNEL_CREATE") => match parse_channel_event(data) {
            Some(ch) => GatewayEvent::ChannelCreate(Box::new(ch)),
            None => GatewayEvent::Unknown {
                op: 0,
                event_name: Some("CHANNEL_CREATE".to_string()),
            },
        },
        Some("CHANNEL_UPDATE") => match parse_channel_event(data) {
            Some(ch) => GatewayEvent::ChannelUpdate(Box::new(ch)),
            None => GatewayEvent::Unknown {
                op: 0,
                event_name: Some("CHANNEL_UPDATE".to_string()),
            },
        },
        Some("CHANNEL_DELETE") => match parse_channel_event(data) {
            Some(ch) => GatewayEvent::ChannelDelete(Box::new(ch)),
            None => GatewayEvent::Unknown {
                op: 0,
                event_name: Some("CHANNEL_DELETE".to_string()),
            },
        },
        Some("TYPING_START") => {
            let channel_id = data["channel_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let user_id = data["user_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let timestamp = data["timestamp"].as_u64().unwrap_or(0);
            if channel_id > 0 && user_id > 0 {
                GatewayEvent::TypingStart {
                    channel_id: Id::new(channel_id),
                    user_id: Id::new(user_id),
                    timestamp,
                }
            } else {
                GatewayEvent::Unknown {
                    op: 0,
                    event_name: Some("TYPING_START".to_string()),
                }
            }
        }
        _ => GatewayEvent::Unknown {
            op: 0,
            event_name: event_name.map(std::string::ToString::to_string),
        },
    }
}

fn parse_message_create(data: &serde_json::Value) -> GatewayEvent {
    let id = data["id"].as_str().and_then(|s| s.parse::<u64>().ok());
    let channel_id = data["channel_id"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok());
    let author_id = data["author"]["id"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok());
    let author_name = data["author"]["username"]
        .as_str()
        .unwrap_or("Unknown")
        .to_string();
    let content = data["content"].as_str().unwrap_or("").to_string();
    let timestamp = data["timestamp"].as_str().unwrap_or("").to_string();
    let mention_everyone = data["mention_everyone"].as_bool().unwrap_or(false);
    let mentions: Vec<Id<UserMarker>> = data["mentions"]
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

    match (id, channel_id, author_id) {
        (Some(id), Some(ch_id), Some(a_id)) => {
            GatewayEvent::MessageCreate(Box::new(MessageCreateEvent {
                id: Id::new(id),
                channel_id: Id::new(ch_id),
                author_id: Id::new(a_id),
                author_name,
                content,
                timestamp,
                mention_everyone,
                mentions,
                raw: data.clone(),
            }))
        }
        _ => GatewayEvent::Unknown {
            op: 0,
            event_name: Some("MESSAGE_CREATE".to_string()),
        },
    }
}

fn parse_message_update(data: &serde_json::Value) -> GatewayEvent {
    let id = data["id"].as_str().and_then(|s| s.parse::<u64>().ok());
    let channel_id = data["channel_id"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok());

    match (id, channel_id) {
        (Some(id), Some(ch_id)) => GatewayEvent::MessageUpdate(Box::new(MessageUpdateEvent {
            id: Id::new(id),
            channel_id: Id::new(ch_id),
            content: data["content"]
                .as_str()
                .map(std::string::ToString::to_string),
            edited_timestamp: data["edited_timestamp"]
                .as_str()
                .map(std::string::ToString::to_string),
            raw: data.clone(),
        })),
        _ => GatewayEvent::Unknown {
            op: 0,
            event_name: Some("MESSAGE_UPDATE".to_string()),
        },
    }
}

fn parse_guild_create(data: &serde_json::Value) -> GatewayEvent {
    let id = data["id"].as_str().and_then(|s| s.parse::<u64>().ok());
    let name = data["name"].as_str().unwrap_or("").to_string();

    match id {
        Some(id) => {
            let channels = data["channels"].as_array().cloned().unwrap_or_default();
            let roles = data["roles"].as_array().cloned().unwrap_or_default();
            GatewayEvent::GuildCreate(Box::new(GuildCreateEvent {
                id: Id::new(id),
                name,
                channels,
                roles,
                raw: data.clone(),
            }))
        }
        None => GatewayEvent::Unknown {
            op: 0,
            event_name: Some("GUILD_CREATE".to_string()),
        },
    }
}

fn parse_channel_event(data: &serde_json::Value) -> Option<ChannelEvent> {
    let id = data["id"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&v| v > 0)?;
    let guild_id = data["guild_id"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Id::new);
    let name = data["name"].as_str().unwrap_or("").to_string();
    let kind = data["type"].as_u64().unwrap_or(0) as u8;
    let position = data["position"].as_i64().unwrap_or(0) as i32;

    Some(ChannelEvent {
        id: Id::new(id),
        guild_id,
        name,
        kind,
        position,
        raw: data.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hello_event() {
        let payload = serde_json::json!({
            "op": 10,
            "d": {"heartbeat_interval": 41250}
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::Hello { heartbeat_interval } => {
                assert_eq!(heartbeat_interval, 41250);
            }
            _ => panic!("Expected Hello event"),
        }
    }

    #[test]
    fn parse_heartbeat_ack() {
        let payload = serde_json::json!({"op": 11, "d": null});
        let event = parse_gateway_payload(&payload);
        assert!(matches!(event, GatewayEvent::HeartbeatAck));
    }

    #[test]
    fn parse_reconnect() {
        let payload = serde_json::json!({"op": 7, "d": null});
        let event = parse_gateway_payload(&payload);
        assert!(matches!(event, GatewayEvent::Reconnect));
    }

    #[test]
    fn parse_invalid_session_resumable() {
        let payload = serde_json::json!({"op": 9, "d": true});
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::InvalidSession { resumable } => assert!(resumable),
            _ => panic!("Expected InvalidSession"),
        }
    }

    #[test]
    fn parse_invalid_session_not_resumable() {
        let payload = serde_json::json!({"op": 9, "d": false});
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::InvalidSession { resumable } => assert!(!resumable),
            _ => panic!("Expected InvalidSession"),
        }
    }

    #[test]
    fn parse_ready_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "READY",
            "s": 1,
            "d": {
                "session_id": "abc123",
                "resume_gateway_url": "wss://gateway.discord.gg",
                "guilds": [{"id": "1", "name": "Test"}],
                "private_channels": [{"id": "10"}],
                "user": {"id": "100", "username": "testuser"},
                "read_state": [],
                "relationships": []
            }
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::Ready(ready) => {
                assert_eq!(ready.session_id, "abc123");
                assert_eq!(ready.resume_gateway_url, "wss://gateway.discord.gg");
                assert_eq!(ready.guilds.len(), 1);
                assert_eq!(ready.private_channels.len(), 1);
            }
            _ => panic!("Expected Ready event"),
        }
    }

    #[test]
    fn parse_resumed_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "RESUMED",
            "s": 5,
            "d": {}
        });
        let event = parse_gateway_payload(&payload);
        assert!(matches!(event, GatewayEvent::Resumed));
    }

    #[test]
    fn parse_message_create_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 10,
            "d": {
                "id": "123456",
                "channel_id": "789",
                "author": {"id": "100", "username": "testuser"},
                "content": "Hello world",
                "timestamp": "2024-01-01T00:00:00Z",
                "mention_everyone": false,
                "mentions": [{"id": "200"}]
            }
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::MessageCreate(msg) => {
                assert_eq!(msg.id.get(), 123456);
                assert_eq!(msg.channel_id.get(), 789);
                assert_eq!(msg.author_id.get(), 100);
                assert_eq!(msg.author_name, "testuser");
                assert_eq!(msg.content, "Hello world");
                assert!(!msg.mention_everyone);
                assert_eq!(msg.mentions.len(), 1);
                assert_eq!(msg.mentions[0].get(), 200);
            }
            _ => panic!("Expected MessageCreate event"),
        }
    }

    #[test]
    fn parse_message_update_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_UPDATE",
            "s": 11,
            "d": {
                "id": "123456",
                "channel_id": "789",
                "content": "Edited content",
                "edited_timestamp": "2024-01-01T01:00:00Z"
            }
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::MessageUpdate(msg) => {
                assert_eq!(msg.id.get(), 123456);
                assert_eq!(msg.channel_id.get(), 789);
                assert_eq!(msg.content, Some("Edited content".to_string()));
                assert_eq!(
                    msg.edited_timestamp,
                    Some("2024-01-01T01:00:00Z".to_string())
                );
            }
            _ => panic!("Expected MessageUpdate event"),
        }
    }

    #[test]
    fn parse_message_delete_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_DELETE",
            "s": 12,
            "d": {
                "id": "123456",
                "channel_id": "789"
            }
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::MessageDelete { id, channel_id } => {
                assert_eq!(id.get(), 123456);
                assert_eq!(channel_id.get(), 789);
            }
            _ => panic!("Expected MessageDelete event"),
        }
    }

    #[test]
    fn parse_guild_create_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "GUILD_CREATE",
            "s": 2,
            "d": {
                "id": "555",
                "name": "Test Server",
                "channels": [{"id": "10", "name": "general"}],
                "roles": [{"id": "20", "name": "Admin"}]
            }
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::GuildCreate(guild) => {
                assert_eq!(guild.id.get(), 555);
                assert_eq!(guild.name, "Test Server");
                assert_eq!(guild.channels.len(), 1);
                assert_eq!(guild.roles.len(), 1);
            }
            _ => panic!("Expected GuildCreate event"),
        }
    }

    #[test]
    fn parse_guild_delete_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "GUILD_DELETE",
            "s": 3,
            "d": {"id": "555"}
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::GuildDelete { id } => assert_eq!(id.get(), 555),
            _ => panic!("Expected GuildDelete event"),
        }
    }

    #[test]
    fn parse_typing_start_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "TYPING_START",
            "s": 20,
            "d": {
                "channel_id": "789",
                "user_id": "100",
                "timestamp": 1704067200
            }
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::TypingStart {
                channel_id,
                user_id,
                timestamp,
            } => {
                assert_eq!(channel_id.get(), 789);
                assert_eq!(user_id.get(), 100);
                assert_eq!(timestamp, 1704067200);
            }
            _ => panic!("Expected TypingStart event"),
        }
    }

    #[test]
    fn parse_channel_create_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "CHANNEL_CREATE",
            "s": 4,
            "d": {
                "id": "10",
                "guild_id": "555",
                "name": "new-channel",
                "type": 0,
                "position": 5
            }
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::ChannelCreate(ch) => {
                assert_eq!(ch.id.get(), 10);
                assert_eq!(ch.guild_id.unwrap().get(), 555);
                assert_eq!(ch.name, "new-channel");
                assert_eq!(ch.kind, 0);
                assert_eq!(ch.position, 5);
            }
            _ => panic!("Expected ChannelCreate event"),
        }
    }

    #[test]
    fn parse_unknown_event() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "SOME_UNKNOWN_EVENT",
            "s": 100,
            "d": {}
        });
        let event = parse_gateway_payload(&payload);
        match event {
            GatewayEvent::Unknown { op, event_name } => {
                assert_eq!(op, 0);
                assert_eq!(event_name, Some("SOME_UNKNOWN_EVENT".to_string()));
            }
            _ => panic!("Expected Unknown event"),
        }
    }

    #[test]
    fn parse_unknown_opcode() {
        let payload = serde_json::json!({"op": 99, "d": null});
        let event = parse_gateway_payload(&payload);
        assert!(matches!(
            event,
            GatewayEvent::Unknown {
                op: 99,
                event_name: None
            }
        ));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // --- P9.1: parse_gateway_payload never panics on arbitrary JSON ---
    proptest! {
        #[test]
        fn parse_never_panics_on_json(op in 0u64..256, has_data in proptest::bool::ANY) {
            let data = if has_data {
                serde_json::json!({"heartbeat_interval": 41250})
            } else {
                serde_json::Value::Null
            };
            let payload = serde_json::json!({"op": op, "d": data});
            let _ = parse_gateway_payload(&payload);
        }
    }

    // --- P9.1 extended: totally arbitrary JSON values ---
    proptest! {
        #[test]
        fn parse_never_panics_on_arbitrary_json(
            s in "[a-zA-Z0-9 {}:,\"\\[\\]]{0,100}"
        ) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&s) {
                let _ = parse_gateway_payload(&val);
            }
        }
    }

    // --- P9.2: known opcodes produce correct event types ---
    proptest! {
        #[test]
        fn known_opcodes_produce_correct_types(heartbeat_interval in 1u64..100000) {
            // op 10 → Hello
            let payload = serde_json::json!({"op": 10, "d": {"heartbeat_interval": heartbeat_interval}});
            let event = parse_gateway_payload(&payload);
            match event {
                GatewayEvent::Hello { heartbeat_interval: hi } => {
                    prop_assert_eq!(hi, heartbeat_interval);
                }
                _ => prop_assert!(false, "Expected Hello, got {:?}", event),
            }

            // op 11 → HeartbeatAck
            let payload = serde_json::json!({"op": 11, "d": null});
            let event = parse_gateway_payload(&payload);
            prop_assert!(matches!(event, GatewayEvent::HeartbeatAck));

            // op 7 → Reconnect
            let payload = serde_json::json!({"op": 7, "d": null});
            let event = parse_gateway_payload(&payload);
            prop_assert!(matches!(event, GatewayEvent::Reconnect));
        }
    }

    // --- P9.2 continued: op 9 → InvalidSession ---
    proptest! {
        #[test]
        fn op9_produces_invalid_session(resumable in proptest::bool::ANY) {
            let payload = serde_json::json!({"op": 9, "d": resumable});
            let event = parse_gateway_payload(&payload);
            match event {
                GatewayEvent::InvalidSession { resumable: r } => {
                    prop_assert_eq!(r, resumable);
                }
                _ => prop_assert!(false, "Expected InvalidSession, got {:?}", event),
            }
        }
    }
}

//! Processes gateway events and background results into cache mutations.
//! Sits between the gateway/HTTP infrastructure and the domain layer.

use std::collections::HashMap;

use tokio::sync::mpsc;
use twilight_model::channel::ChannelType;

use crate::app::AppState;
use crate::domain::event::GatewayEvent;
use crate::domain::types::{
    BackgroundResult, CachedChannel, CachedGuild, CachedMessage, CachedRole, CachedUser,
    ConnectionState, DbRequest, Id, MessageAttachment, MessageEmbed,
    MessageReference, RoleMarker,
};

/// Process a gateway event, update app state and cache. Returns true if dirty.
#[allow(clippy::too_many_lines)]
pub fn handle_gateway_event(
    event: GatewayEvent,
    state: &mut AppState,
    db_tx: &mpsc::Sender<DbRequest>,
) -> bool {
    match event {
        GatewayEvent::Ready(ready) => {
            state.connection = ConnectionState::Connected {
                session_id: ready.session_id.clone(),
                resume_url: ready.resume_gateway_url.clone(),
                sequence: 0,
            };

            // Parse current user
            if let Some(uid) = parse_id(&ready.user["id"]) {
                state.current_user_id = Some(Id::new(uid));
                state.cache.users.insert(
                    Id::new(uid),
                    CachedUser {
                        id: Id::new(uid),
                        name: ready.user["username"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                        discriminator: None,
                        display_name: ready.user["global_name"]
                            .as_str()
                            .map(String::from),
                        avatar: ready.user["avatar"].as_str().map(String::from),
                    },
                );
            }

            // Parse guilds from READY (these are partial; full data comes via GUILD_CREATE)
            for guild_json in &ready.guilds {
                if let Some(guild) = parse_guild_json(guild_json) {
                    // Parse channels if present
                    if let Some(channels) = guild_json["channels"].as_array() {
                        for ch_json in channels {
                            if let Some(channel) = parse_channel_json(ch_json) {
                                state.cache.insert_channel(channel);
                            }
                        }
                    }
                    state.cache.insert_guild(guild);
                }
            }

            // Parse DM channels
            for dm_json in &ready.private_channels {
                if let Some(dm) = parse_dm_channel_json(dm_json) {
                    state.cache.dm_channels.push(dm.id);
                    state.cache.insert_channel(dm);
                }
            }

            state.status_message = Some("Connected".to_string());
            state.status_error = None;
            true
        }

        GatewayEvent::Resumed => {
            state.status_message = Some("Resumed".to_string());
            true
        }

        GatewayEvent::MessageCreate(msg) => {
            // Cache the author
            state
                .cache
                .users
                .entry(msg.author_id)
                .or_insert_with(|| CachedUser {
                    id: msg.author_id,
                    name: msg.author_name.clone(),
                    discriminator: None,
                    display_name: None,
                    avatar: None,
                });

            let cached = CachedMessage {
                id: msg.id,
                channel_id: msg.channel_id,
                author_id: msg.author_id,
                content: msg.content.clone(),
                timestamp: msg.timestamp.clone(),
                edited_timestamp: None,
                attachments: parse_attachments_json(&msg.raw),
                embeds: parse_embeds_json(&msg.raw),
                message_reference: parse_message_reference_json(&msg.raw),
                mention_everyone: msg.mention_everyone,
                mentions: msg.mentions.clone(),
                rendered: None,
            };

            let _ = db_tx.try_send(DbRequest::InsertMessage(cached.clone()));
            state.cache.insert_message(cached);
            true
        }

        GatewayEvent::MessageUpdate(update) => {
            if let Some(content) = &update.content {
                state.cache.update_message(
                    update.channel_id,
                    update.id,
                    content.clone(),
                    update.edited_timestamp.clone(),
                );
                if let Some(edited_ts) = &update.edited_timestamp {
                    let _ = db_tx.try_send(DbRequest::UpdateMessage {
                        id: update.id,
                        content: content.clone(),
                        edited_timestamp: edited_ts.clone(),
                    });
                }
            }
            true
        }

        GatewayEvent::MessageDelete { id, channel_id } => {
            state.cache.delete_message(channel_id, id);
            let _ = db_tx.try_send(DbRequest::DeleteMessage(id));
            true
        }

        GatewayEvent::GuildCreate(guild) => {
            let roles = parse_roles_json(&guild.roles);
            let cached_guild = CachedGuild {
                id: guild.id,
                name: guild.name.clone(),
                icon: guild.raw["icon"].as_str().map(String::from),
                channel_order: vec![],
                roles,
            };
            state.cache.insert_guild(cached_guild);

            for ch_json in &guild.channels {
                if let Some(channel) = parse_channel_json(ch_json) {
                    let ch_id = channel.id;
                    state.cache.insert_channel(channel);
                    if let Some(g) = state.cache.guilds.get_mut(&guild.id) {
                        if !g.channel_order.contains(&ch_id) {
                            g.channel_order.push(ch_id);
                        }
                    }
                }
            }
            true
        }

        GatewayEvent::GuildDelete { id } => {
            state.cache.remove_guild(id);
            true
        }

        GatewayEvent::ChannelCreate(ch) | GatewayEvent::ChannelUpdate(ch) => {
            let channel = CachedChannel {
                id: ch.id,
                guild_id: ch.guild_id,
                name: ch.name.clone(),
                kind: channel_type_from_u8(ch.kind),
                position: ch.position,
                parent_id: ch.raw["parent_id"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(Id::new),
                topic: ch.raw["topic"].as_str().map(String::from),
            };
            state.cache.insert_channel(channel);
            true
        }

        GatewayEvent::ChannelDelete(ch) => {
            state.cache.remove_channel(ch.id);
            true
        }

        GatewayEvent::TypingStart {
            channel_id,
            user_id,
            ..
        } => {
            let now = std::time::Instant::now();
            let typers = state.cache.typing.entry(channel_id).or_default();
            // Remove expired entries (> 10 seconds)
            typers.retain(|(_, t)| now.duration_since(*t).as_secs() < 10);
            if let Some(entry) = typers.iter_mut().find(|(uid, _)| *uid == user_id) {
                entry.1 = now;
            } else {
                typers.push((user_id, now));
            }
            true
        }

        GatewayEvent::InvalidSession { .. } => {
            state.connection = ConnectionState::Disconnected;
            state.status_error = Some("Session invalidated, reconnecting...".to_string());
            true
        }

        GatewayEvent::Reconnect => {
            state.connection = ConnectionState::Connecting;
            state.status_message = Some("Reconnecting...".to_string());
            true
        }

        // Hello and HeartbeatAck are handled internally by GatewayConnection
        GatewayEvent::Hello { .. }
        | GatewayEvent::HeartbeatAck
        | GatewayEvent::Unknown { .. } => false,
    }
}

/// Process a background result (HTTP fetch, DB cache, errors). Returns true if dirty.
pub fn handle_background_result(result: BackgroundResult, state: &mut AppState) -> bool {
    match result {
        BackgroundResult::MessagesFetched {
            channel_id,
            messages,
        } => {
            state.cache.prepend_messages(channel_id, messages);
            true
        }
        BackgroundResult::CachedMessages {
            channel_id,
            messages,
        } => {
            // Only use DB cache if we don't already have messages for this channel
            if state
                .cache
                .messages
                .get(&channel_id)
                .is_none_or(std::collections::VecDeque::is_empty)
            {
                state.cache.prepend_messages(channel_id, messages);
                true
            } else {
                false
            }
        }
        BackgroundResult::HttpError { request, error } => {
            tracing::error!("HTTP error: {} - {}", request, error);
            state.status_error = Some(format!("HTTP: {error}"));
            true
        }
        BackgroundResult::SessionLoaded { layout_json, .. } => {
            if layout_json.is_some() {
                // TODO: restore pane layout from JSON
                tracing::info!("Session loaded (layout restore not yet implemented)");
            }
            false
        }
        BackgroundResult::DbError { operation, error } => {
            tracing::error!("DB error in {}: {}", operation, error);
            false
        }
    }
}

// === JSON parsing helpers for READY event data ===

fn parse_id(val: &serde_json::Value) -> Option<u64> {
    val.as_str().and_then(|s| s.parse::<u64>().ok())
}

fn parse_guild_json(json: &serde_json::Value) -> Option<CachedGuild> {
    let id = parse_id(&json["id"])?;
    let name = json["name"].as_str().unwrap_or("").to_string();
    let icon = json["icon"].as_str().map(String::from);

    let roles = json["roles"]
        .as_array()
        .map(|arr| parse_roles_json(arr))
        .unwrap_or_default();

    let channel_order = json["channels"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|ch| parse_id(&ch["id"]).map(Id::new))
                .collect()
        })
        .unwrap_or_default();

    Some(CachedGuild {
        id: Id::new(id),
        name,
        icon,
        channel_order,
        roles,
    })
}

fn parse_channel_json(json: &serde_json::Value) -> Option<CachedChannel> {
    let id = parse_id(&json["id"])?;
    let guild_id = parse_id(&json["guild_id"]).map(Id::new);
    let name = json["name"].as_str().unwrap_or("").to_string();
    let kind = json["type"].as_u64().unwrap_or(0) as u8;
    let position = json["position"].as_i64().unwrap_or(0) as i32;
    let parent_id = parse_id(&json["parent_id"]).map(Id::new);
    let topic = json["topic"].as_str().map(String::from);

    Some(CachedChannel {
        id: Id::new(id),
        guild_id,
        name,
        kind: channel_type_from_u8(kind),
        position,
        parent_id,
        topic,
    })
}

fn parse_dm_channel_json(json: &serde_json::Value) -> Option<CachedChannel> {
    let id = parse_id(&json["id"])?;

    // DM channel names: use the recipient's username
    let name = json["recipients"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|r| r["username"].as_str())
        .unwrap_or("DM")
        .to_string();

    let kind = json["type"].as_u64().unwrap_or(1) as u8;

    Some(CachedChannel {
        id: Id::new(id),
        guild_id: None,
        name,
        kind: channel_type_from_u8(kind),
        position: 0,
        parent_id: None,
        topic: None,
    })
}

fn parse_roles_json(roles: &[serde_json::Value]) -> HashMap<Id<RoleMarker>, CachedRole> {
    let mut map = HashMap::new();
    for r in roles {
        if let Some(id) = parse_id(&r["id"]) {
            map.insert(
                Id::new(id),
                CachedRole {
                    id: Id::new(id),
                    name: r["name"].as_str().unwrap_or("").to_string(),
                    color: r["color"].as_u64().unwrap_or(0) as u32,
                    position: r["position"].as_i64().unwrap_or(0) as i32,
                },
            );
        }
    }
    map
}

fn parse_attachments_json(raw: &serde_json::Value) -> Vec<MessageAttachment> {
    raw["attachments"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some(MessageAttachment {
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
        .unwrap_or_default()
}

fn parse_embeds_json(raw: &serde_json::Value) -> Vec<MessageEmbed> {
    raw["embeds"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|e| MessageEmbed {
                    title: e["title"]
                        .as_str()
                        .map(std::string::ToString::to_string),
                    description: e["description"]
                        .as_str()
                        .map(std::string::ToString::to_string),
                    color: e["color"].as_u64().map(|c| c as u32),
                    url: e["url"].as_str().map(std::string::ToString::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_message_reference_json(raw: &serde_json::Value) -> Option<MessageReference> {
    if !raw["message_reference"].is_object() {
        return None;
    }
    let r = &raw["message_reference"];
    Some(MessageReference {
        message_id: parse_id(&r["message_id"]).map(Id::new),
        channel_id: parse_id(&r["channel_id"]).map(Id::new),
        guild_id: parse_id(&r["guild_id"]).map(Id::new),
    })
}

fn channel_type_from_u8(kind: u8) -> ChannelType {
    match kind {
        1 => ChannelType::Private,
        2 => ChannelType::GuildVoice,
        3 => ChannelType::Group,
        4 => ChannelType::GuildCategory,
        5 => ChannelType::GuildAnnouncement,
        13 => ChannelType::GuildStageVoice,
        15 => ChannelType::GuildForum,
        _ => ChannelType::GuildText, // Default fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::domain::event::{
        GuildCreateEvent, MessageCreateEvent, MessageUpdateEvent, ReadyEvent,
    };

    fn test_state() -> AppState {
        AppState::new(AppConfig::default())
    }

    fn make_db_tx() -> (mpsc::Sender<DbRequest>, mpsc::Receiver<DbRequest>) {
        mpsc::channel(64)
    }

    #[test]
    fn handle_ready_sets_connected() {
        let mut state = test_state();
        let (db_tx, _db_rx) = make_db_tx();
        let ready = ReadyEvent {
            session_id: "sess123".to_string(),
            resume_gateway_url: "wss://resume.test".to_string(),
            guilds: vec![],
            private_channels: vec![],
            read_state: vec![],
            relationships: vec![],
            user: serde_json::json!({"id": "42", "username": "testuser"}),
        };
        let dirty = handle_gateway_event(
            GatewayEvent::Ready(Box::new(ready)),
            &mut state,
            &db_tx,
        );
        assert!(dirty);
        assert!(matches!(
            state.connection,
            ConnectionState::Connected { .. }
        ));
        assert_eq!(state.current_user_id, Some(Id::new(42)));
    }

    #[test]
    fn handle_ready_parses_guilds_and_channels() {
        let mut state = test_state();
        let (db_tx, _db_rx) = make_db_tx();
        let ready = ReadyEvent {
            session_id: "s".to_string(),
            resume_gateway_url: "wss://r".to_string(),
            guilds: vec![serde_json::json!({
                "id": "1",
                "name": "Test Server",
                "channels": [
                    {"id": "10", "name": "general", "type": 0, "position": 0},
                    {"id": "20", "name": "random", "type": 0, "position": 1}
                ],
                "roles": [{"id": "100", "name": "Admin", "color": 255, "position": 1}]
            })],
            private_channels: vec![serde_json::json!({
                "id": "999",
                "type": 1,
                "recipients": [{"id": "50", "username": "friend"}]
            })],
            read_state: vec![],
            relationships: vec![],
            user: serde_json::json!({"id": "42", "username": "me"}),
        };
        handle_gateway_event(
            GatewayEvent::Ready(Box::new(ready)),
            &mut state,
            &db_tx,
        );

        assert!(state.cache.guilds.contains_key(&Id::new(1)));
        assert!(state.cache.channels.contains_key(&Id::new(10)));
        assert!(state.cache.channels.contains_key(&Id::new(20)));
        assert!(state.cache.channels.contains_key(&Id::new(999)));
        assert!(state.cache.dm_channels.contains(&Id::new(999)));
        assert_eq!(
            state.cache.channels.get(&Id::new(999)).unwrap().name,
            "friend"
        );
    }

    #[test]
    fn handle_message_create_caches_message() {
        let mut state = test_state();
        let (db_tx, mut db_rx) = make_db_tx();
        let msg = MessageCreateEvent {
            id: Id::new(100),
            channel_id: Id::new(10),
            author_id: Id::new(42),
            author_name: "testuser".to_string(),
            content: "Hello world".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            mention_everyone: false,
            mentions: vec![],
            raw: serde_json::json!({
                "attachments": [],
                "embeds": []
            }),
        };
        let dirty = handle_gateway_event(
            GatewayEvent::MessageCreate(Box::new(msg)),
            &mut state,
            &db_tx,
        );
        assert!(dirty);
        let msgs = state.cache.messages.get(&Id::new(10)).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "Hello world");

        // Should also send DB insert
        let db_req = db_rx.try_recv().unwrap();
        assert!(matches!(db_req, DbRequest::InsertMessage(_)));
    }

    #[test]
    fn handle_message_update_updates_cache() {
        let mut state = test_state();
        let (db_tx, _db_rx) = make_db_tx();
        // Insert a message first
        state.cache.insert_message(CachedMessage {
            id: Id::new(100),
            channel_id: Id::new(10),
            author_id: Id::new(42),
            content: "original".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        });

        let update = crate::domain::event::MessageUpdateEvent {
            id: Id::new(100),
            channel_id: Id::new(10),
            content: Some("updated".to_string()),
            edited_timestamp: Some("2024-01-01T01:00:00Z".to_string()),
            raw: serde_json::json!({}),
        };
        let dirty = handle_gateway_event(
            GatewayEvent::MessageUpdate(Box::new(update)),
            &mut state,
            &db_tx,
        );
        assert!(dirty);
        let msgs = state.cache.messages.get(&Id::new(10)).unwrap();
        assert_eq!(msgs[0].content, "updated");
    }

    #[test]
    fn handle_message_delete_removes_from_cache() {
        let mut state = test_state();
        let (db_tx, _db_rx) = make_db_tx();
        state.cache.insert_message(CachedMessage {
            id: Id::new(100),
            channel_id: Id::new(10),
            author_id: Id::new(42),
            content: "to delete".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        });

        let dirty = handle_gateway_event(
            GatewayEvent::MessageDelete {
                id: Id::new(100),
                channel_id: Id::new(10),
            },
            &mut state,
            &db_tx,
        );
        assert!(dirty);
        assert!(state
            .cache
            .messages
            .get(&Id::new(10))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn handle_guild_create_caches_guild_and_channels() {
        let mut state = test_state();
        let (db_tx, _db_rx) = make_db_tx();
        let guild = GuildCreateEvent {
            id: Id::new(1),
            name: "New Server".to_string(),
            channels: vec![serde_json::json!({
                "id": "10", "name": "general", "type": 0, "position": 0
            })],
            roles: vec![],
            raw: serde_json::json!({}),
        };
        let dirty = handle_gateway_event(
            GatewayEvent::GuildCreate(Box::new(guild)),
            &mut state,
            &db_tx,
        );
        assert!(dirty);
        assert!(state.cache.guilds.contains_key(&Id::new(1)));
        assert!(state.cache.channels.contains_key(&Id::new(10)));
    }

    #[test]
    fn handle_guild_delete_removes_guild() {
        let mut state = test_state();
        let (db_tx, _db_rx) = make_db_tx();
        state.cache.insert_guild(CachedGuild {
            id: Id::new(1),
            name: "Test".to_string(),
            icon: None,
            channel_order: vec![],
            roles: HashMap::new(),
        });
        let dirty = handle_gateway_event(
            GatewayEvent::GuildDelete { id: Id::new(1) },
            &mut state,
            &db_tx,
        );
        assert!(dirty);
        assert!(!state.cache.guilds.contains_key(&Id::new(1)));
    }

    #[test]
    fn handle_invalid_session_disconnects() {
        let mut state = test_state();
        let (db_tx, _db_rx) = make_db_tx();
        state.connection = ConnectionState::Connected {
            session_id: "s".to_string(),
            resume_url: "wss://r".to_string(),
            sequence: 10,
        };
        let dirty = handle_gateway_event(
            GatewayEvent::InvalidSession { resumable: false },
            &mut state,
            &db_tx,
        );
        assert!(dirty);
        assert_eq!(state.connection, ConnectionState::Disconnected);
    }

    #[test]
    fn handle_typing_start_tracks_typer() {
        let mut state = test_state();
        let (db_tx, _db_rx) = make_db_tx();
        let dirty = handle_gateway_event(
            GatewayEvent::TypingStart {
                channel_id: Id::new(10),
                user_id: Id::new(42),
                timestamp: 1234567890,
            },
            &mut state,
            &db_tx,
        );
        assert!(dirty);
        let typers = state.cache.typing.get(&Id::new(10)).unwrap();
        assert_eq!(typers.len(), 1);
        assert_eq!(typers[0].0, Id::new(42));
    }

    #[test]
    fn handle_background_messages_fetched() {
        let mut state = test_state();
        let msgs = vec![CachedMessage {
            id: Id::new(1),
            channel_id: Id::new(10),
            author_id: Id::new(42),
            content: "fetched".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        }];
        let dirty = handle_background_result(
            BackgroundResult::MessagesFetched {
                channel_id: Id::new(10),
                messages: msgs,
            },
            &mut state,
        );
        assert!(dirty);
        assert_eq!(
            state.cache.messages.get(&Id::new(10)).unwrap().len(),
            1
        );
    }

    #[test]
    fn handle_background_cached_messages_only_when_empty() {
        let mut state = test_state();
        // Pre-populate with a message
        state.cache.insert_message(CachedMessage {
            id: Id::new(1),
            channel_id: Id::new(10),
            author_id: Id::new(42),
            content: "existing".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        });

        let msgs = vec![CachedMessage {
            id: Id::new(2),
            channel_id: Id::new(10),
            author_id: Id::new(42),
            content: "cached".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        }];
        let dirty = handle_background_result(
            BackgroundResult::CachedMessages {
                channel_id: Id::new(10),
                messages: msgs,
            },
            &mut state,
        );
        // Should NOT insert because channel already has messages
        assert!(!dirty);
        assert_eq!(
            state.cache.messages.get(&Id::new(10)).unwrap().len(),
            1
        );
    }

    #[test]
    fn handle_background_http_error() {
        let mut state = test_state();
        let dirty = handle_background_result(
            BackgroundResult::HttpError {
                request: "SendMessage".to_string(),
                error: "rate limited".to_string(),
            },
            &mut state,
        );
        assert!(dirty);
        assert!(state.status_error.is_some());
    }

    #[test]
    fn parse_guild_json_basic() {
        let json = serde_json::json!({
            "id": "123",
            "name": "Test Guild",
            "icon": "abc",
            "roles": [],
            "channels": [{"id": "10"}, {"id": "20"}]
        });
        let guild = parse_guild_json(&json).unwrap();
        assert_eq!(guild.id, Id::new(123));
        assert_eq!(guild.name, "Test Guild");
        assert_eq!(guild.channel_order.len(), 2);
    }

    #[test]
    fn parse_channel_json_basic() {
        let json = serde_json::json!({
            "id": "10",
            "guild_id": "1",
            "name": "general",
            "type": 0,
            "position": 5,
            "topic": "Welcome!"
        });
        let ch = parse_channel_json(&json).unwrap();
        assert_eq!(ch.id, Id::new(10));
        assert_eq!(ch.guild_id, Some(Id::new(1)));
        assert_eq!(ch.name, "general");
        assert_eq!(ch.position, 5);
        assert_eq!(ch.topic, Some("Welcome!".to_string()));
    }

    #[test]
    fn parse_dm_channel_json_basic() {
        let json = serde_json::json!({
            "id": "999",
            "type": 1,
            "recipients": [{"id": "50", "username": "friend"}]
        });
        let dm = parse_dm_channel_json(&json).unwrap();
        assert_eq!(dm.id, Id::new(999));
        assert_eq!(dm.name, "friend");
        assert!(dm.guild_id.is_none());
    }

    #[test]
    fn channel_type_from_u8_variants() {
        assert_eq!(channel_type_from_u8(0), ChannelType::GuildText);
        assert_eq!(channel_type_from_u8(1), ChannelType::Private);
        assert_eq!(channel_type_from_u8(2), ChannelType::GuildVoice);
        assert_eq!(channel_type_from_u8(4), ChannelType::GuildCategory);
        assert_eq!(channel_type_from_u8(255), ChannelType::GuildText); // fallback
    }
}

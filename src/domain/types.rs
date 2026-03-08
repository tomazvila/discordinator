use serde::{Deserialize, Serialize};

// Re-exports from twilight-model
pub use twilight_model::id::{
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
    Id,
};

/// Maximum messages kept in memory per channel.
pub const MAX_CACHED_MESSAGES_PER_CHANNEL: usize = 200;

/// Newtype for pane IDs - prevents mixing with Discord IDs.
/// Discord IDs are Id<T> (`NonZeroU64`), `PaneId` is u32 - completely disjoint types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaneId(pub u32);

/// Connection state machine - makes impossible states unrepresentable.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected {
        session_id: String,
        resume_url: String,
        sequence: u64,
    },
    Resuming {
        session_id: String,
        resume_url: String,
        sequence: u64,
    },
}

/// Cached guild data.
#[derive(Debug, Clone)]
pub struct CachedGuild {
    pub id: Id<GuildMarker>,
    pub name: String,
    pub icon: Option<String>,
    pub channel_order: Vec<Id<ChannelMarker>>,
    pub roles: std::collections::HashMap<Id<RoleMarker>, CachedRole>,
}

/// Cached channel data.
#[derive(Debug, Clone)]
pub struct CachedChannel {
    pub id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub name: String,
    pub kind: twilight_model::channel::ChannelType,
    pub position: i32,
    pub parent_id: Option<Id<ChannelMarker>>,
    pub topic: Option<String>,
}

/// Cached user data.
#[derive(Debug, Clone)]
pub struct CachedUser {
    pub id: Id<UserMarker>,
    pub name: String,
    pub discriminator: Option<u16>,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
}

/// Cached role data.
#[derive(Debug, Clone)]
pub struct CachedRole {
    pub id: Id<RoleMarker>,
    pub name: String,
    pub color: u32,
    pub position: i32,
}

/// Cached message data.
#[derive(Debug, Clone)]
pub struct CachedMessage {
    pub id: Id<MessageMarker>,
    pub channel_id: Id<ChannelMarker>,
    pub author_id: Id<UserMarker>,
    pub content: String,
    pub timestamp: String,
    pub edited_timestamp: Option<String>,
    pub attachments: Vec<MessageAttachment>,
    pub embeds: Vec<MessageEmbed>,
    pub message_reference: Option<MessageReference>,
    pub mention_everyone: bool,
    pub mentions: Vec<Id<UserMarker>>,
    /// Cached rendered output - lazily computed, invalidated on edit.
    pub rendered: Option<Vec<ratatui::text::Line<'static>>>,
}

/// Message attachment metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttachment {
    pub filename: String,
    pub size: u64,
    pub url: String,
    pub content_type: Option<String>,
}

/// Simplified embed data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEmbed {
    pub title: Option<String>,
    pub description: Option<String>,
    pub color: Option<u32>,
    pub url: Option<String>,
}

/// Message reference (for replies).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReference {
    pub message_id: Option<Id<MessageMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub guild_id: Option<Id<GuildMarker>>,
}

/// Per-channel read state.
#[derive(Debug, Clone)]
pub struct ReadState {
    pub last_message_id: Id<MessageMarker>,
    pub mention_count: u32,
}

/// Direction for pane operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Pane split direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Scroll state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum ScrollState {
    Following,
    Manual { offset: usize },
}

/// Input state for a pane's input box.
#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub content: String,
    pub cursor_pos: usize,
    pub cursor_col: usize,
    pub reply_to: Option<Id<MessageMarker>>,
    pub editing: Option<Id<MessageMarker>>,
}

/// All state mutations flow through Action.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // Navigation
    SwitchChannel(Id<ChannelMarker>),
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToTop,
    ScrollToBottom,

    // Messages
    SendMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
        reply_to: Option<Id<MessageMarker>>,
    },
    EditMessage {
        message_id: Id<MessageMarker>,
        content: String,
    },
    DeleteMessage {
        message_id: Id<MessageMarker>,
        channel_id: Id<ChannelMarker>,
    },

    // Input mode
    EnterInsertMode,
    EnterNormalMode,
    EnterCommandMode,
    EnterPanePrefix,

    // Pane operations
    SplitPane(SplitDirection),
    ClosePane,
    FocusNextPane,
    FocusPaneDirection(Direction),
    ResizePane(Direction, i16),
    ToggleZoom,
    SwapPane(Direction),

    // UI toggles
    ToggleSidebar,
    ToggleCommandPalette,

    // Sidebar navigation
    SidebarNavigateUp,
    SidebarNavigateDown,
    SidebarSelect,
    SidebarCollapse,
    SidebarToggleCollapse,
    FocusSidebar,
    FocusPaneArea,

    // Message interaction
    StartReply,
    StartEdit,
    StartDelete,
    ConfirmDelete,
    CancelDelete,
    SelectMessageUp,
    SelectMessageDown,

    // System
    Quit,
    ForceQuit,
}

/// Requests sent to the HTTP actor task.
#[derive(Debug, Clone)]
pub enum HttpRequest {
    SendMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
        nonce: String,
        reply_to: Option<Id<MessageMarker>>,
    },
    EditMessage {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: String,
    },
    DeleteMessage {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    FetchMessages {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u8,
    },
    SendTyping {
        channel_id: Id<ChannelMarker>,
    },
}

/// Requests sent to the `SQLite` worker task.
#[derive(Debug, Clone)]
pub enum DbRequest {
    InsertMessage(CachedMessage),
    InsertMessages(Vec<CachedMessage>),
    UpdateMessage {
        id: Id<MessageMarker>,
        content: String,
        edited_timestamp: String,
    },
    DeleteMessage(Id<MessageMarker>),
    FetchMessages {
        channel_id: Id<ChannelMarker>,
        before_timestamp: Option<String>,
        limit: u32,
    },
    SaveSession {
        name: String,
        layout_json: String,
    },
    LoadSession {
        name: String,
    },
}

/// Results from background tasks back to main loop.
#[derive(Debug, Clone)]
pub enum BackgroundResult {
    MessagesFetched {
        channel_id: Id<ChannelMarker>,
        messages: Vec<CachedMessage>,
    },
    HttpError {
        request: String,
        error: String,
    },
    CachedMessages {
        channel_id: Id<ChannelMarker>,
        messages: Vec<CachedMessage>,
    },
    SessionLoaded {
        name: String,
        layout_json: Option<String>,
    },
    DbError {
        operation: String,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_id_is_distinct_type() {
        let pane = PaneId(1);
        let pane2 = PaneId(2);
        assert_ne!(pane, pane2);
        assert_eq!(pane, PaneId(1));
    }

    #[test]
    fn pane_id_is_copy_and_hash() {
        use std::collections::HashSet;
        let pane = PaneId(42);
        let pane_copy = pane; // Copy
        assert_eq!(pane, pane_copy);

        let mut set = HashSet::new();
        set.insert(pane);
        assert!(set.contains(&PaneId(42)));
    }

    #[test]
    fn discord_ids_are_type_safe() {
        // These are different types at compile time
        let user_id: Id<UserMarker> = Id::new(123);
        let channel_id: Id<ChannelMarker> = Id::new(123);
        let guild_id: Id<GuildMarker> = Id::new(123);
        let message_id: Id<MessageMarker> = Id::new(123);

        // Same underlying value, but different types
        assert_eq!(user_id.get(), channel_id.get());
        assert_eq!(guild_id.get(), message_id.get());

        // Verify they're usable as HashMap keys
        use std::collections::HashMap;
        let mut map: HashMap<Id<UserMarker>, String> = HashMap::new();
        map.insert(user_id, "test".to_string());
        assert_eq!(map.get(&user_id), Some(&"test".to_string()));
    }

    #[test]
    fn connection_state_variants() {
        let disconnected = ConnectionState::Disconnected;
        let connecting = ConnectionState::Connecting;
        let connected = ConnectionState::Connected {
            session_id: "abc".to_string(),
            resume_url: "wss://example.com".to_string(),
            sequence: 42,
        };
        let resuming = ConnectionState::Resuming {
            session_id: "abc".to_string(),
            resume_url: "wss://example.com".to_string(),
            sequence: 42,
        };

        assert_eq!(disconnected, ConnectionState::Disconnected);
        assert_eq!(connecting, ConnectionState::Connecting);
        assert_ne!(disconnected, connecting);

        // Verify Connected carries data
        if let ConnectionState::Connected {
            session_id,
            sequence,
            ..
        } = &connected
        {
            assert_eq!(session_id, "abc");
            assert_eq!(*sequence, 42);
        } else {
            panic!("Expected Connected variant");
        }

        // Verify Resuming carries data
        if let ConnectionState::Resuming {
            session_id,
            sequence,
            ..
        } = &resuming
        {
            assert_eq!(session_id, "abc");
            assert_eq!(*sequence, 42);
        } else {
            panic!("Expected Resuming variant");
        }
    }

    #[test]
    fn cached_message_construction() {
        let msg = CachedMessage {
            id: Id::new(1),
            channel_id: Id::new(2),
            author_id: Id::new(3),
            content: "Hello world".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        };

        assert_eq!(msg.content, "Hello world");
        assert_eq!(msg.id.get(), 1);
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn cached_message_with_attachments_and_embeds() {
        let msg = CachedMessage {
            id: Id::new(1),
            channel_id: Id::new(2),
            author_id: Id::new(3),
            content: "Check this out".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: Some("2024-01-01T01:00:00Z".to_string()),
            attachments: vec![MessageAttachment {
                filename: "photo.png".to_string(),
                size: 1024,
                url: "https://cdn.example.com/photo.png".to_string(),
                content_type: Some("image/png".to_string()),
            }],
            embeds: vec![MessageEmbed {
                title: Some("Link Title".to_string()),
                description: Some("Description".to_string()),
                color: Some(0xFF0000),
                url: Some("https://example.com".to_string()),
            }],
            message_reference: Some(MessageReference {
                message_id: Some(Id::new(100)),
                channel_id: Some(Id::new(2)),
                guild_id: None,
            }),
            mention_everyone: false,
            mentions: vec![Id::new(10), Id::new(20)],
            rendered: None,
        };

        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "photo.png");
        assert_eq!(msg.embeds.len(), 1);
        assert_eq!(msg.embeds[0].title, Some("Link Title".to_string()));
        assert!(msg.message_reference.is_some());
        assert_eq!(msg.mentions.len(), 2);
        assert!(msg.edited_timestamp.is_some());
    }

    #[test]
    fn action_variants_constructible() {
        let actions = vec![
            Action::SwitchChannel(Id::new(1)),
            Action::ScrollUp(10),
            Action::ScrollDown(10),
            Action::ScrollToTop,
            Action::ScrollToBottom,
            Action::SendMessage {
                channel_id: Id::new(1),
                content: "hello".to_string(),
                reply_to: None,
            },
            Action::EditMessage {
                message_id: Id::new(1),
                content: "edited".to_string(),
            },
            Action::DeleteMessage {
                message_id: Id::new(1),
                channel_id: Id::new(2),
            },
            Action::EnterInsertMode,
            Action::EnterNormalMode,
            Action::EnterCommandMode,
            Action::EnterPanePrefix,
            Action::SplitPane(SplitDirection::Horizontal),
            Action::SplitPane(SplitDirection::Vertical),
            Action::ClosePane,
            Action::FocusNextPane,
            Action::FocusPaneDirection(Direction::Up),
            Action::ResizePane(Direction::Left, 5),
            Action::ToggleZoom,
            Action::SwapPane(Direction::Right),
            Action::ToggleSidebar,
            Action::ToggleCommandPalette,
            Action::SidebarNavigateUp,
            Action::SidebarNavigateDown,
            Action::SidebarSelect,
            Action::SidebarCollapse,
            Action::SidebarToggleCollapse,
            Action::FocusSidebar,
            Action::FocusPaneArea,
            Action::StartReply,
            Action::StartEdit,
            Action::StartDelete,
            Action::ConfirmDelete,
            Action::CancelDelete,
            Action::SelectMessageUp,
            Action::SelectMessageDown,
            Action::Quit,
            Action::ForceQuit,
        ];
        assert_eq!(actions.len(), 38);
    }

    #[test]
    fn scroll_state_variants() {
        let following = ScrollState::Following;
        let manual = ScrollState::Manual { offset: 42 };

        assert_eq!(following, ScrollState::Following);
        if let ScrollState::Manual { offset } = manual {
            assert_eq!(offset, 42);
        } else {
            panic!("Expected Manual variant");
        }
    }

    #[test]
    fn input_state_default() {
        let input = InputState::default();
        assert!(input.content.is_empty());
        assert_eq!(input.cursor_pos, 0);
        assert_eq!(input.cursor_col, 0);
        assert!(input.reply_to.is_none());
        assert!(input.editing.is_none());
    }

    #[test]
    fn http_request_variants() {
        let req = HttpRequest::SendMessage {
            channel_id: Id::new(1),
            content: "hello".to_string(),
            nonce: "abc123".to_string(),
            reply_to: Some(Id::new(99)),
        };
        if let HttpRequest::SendMessage {
            channel_id,
            reply_to,
            ..
        } = req
        {
            assert_eq!(channel_id.get(), 1);
            assert_eq!(reply_to.unwrap().get(), 99);
        }
    }

    #[test]
    fn db_request_variants() {
        let req = DbRequest::FetchMessages {
            channel_id: Id::new(1),
            before_timestamp: None,
            limit: 50,
        };
        if let DbRequest::FetchMessages { limit, .. } = req {
            assert_eq!(limit, 50);
        }
    }

    #[test]
    fn background_result_variants() {
        let result = BackgroundResult::HttpError {
            request: "SendMessage".to_string(),
            error: "rate limited".to_string(),
        };
        if let BackgroundResult::HttpError { request, error } = result {
            assert_eq!(request, "SendMessage");
            assert_eq!(error, "rate limited");
        }
    }

    #[test]
    fn max_cached_messages_constant() {
        assert_eq!(MAX_CACHED_MESSAGES_PER_CHANNEL, 200);
    }

    #[test]
    fn cached_guild_construction() {
        let guild = CachedGuild {
            id: Id::new(1),
            name: "Test Server".to_string(),
            icon: Some("abc123".to_string()),
            channel_order: vec![Id::new(10), Id::new(20)],
            roles: std::collections::HashMap::new(),
        };
        assert_eq!(guild.name, "Test Server");
        assert_eq!(guild.channel_order.len(), 2);
    }

    #[test]
    fn cached_channel_construction() {
        let channel = CachedChannel {
            id: Id::new(10),
            guild_id: Some(Id::new(1)),
            name: "general".to_string(),
            kind: twilight_model::channel::ChannelType::GuildText,
            position: 0,
            parent_id: None,
            topic: Some("General chat".to_string()),
        };
        assert_eq!(channel.name, "general");
        assert_eq!(
            channel.kind,
            twilight_model::channel::ChannelType::GuildText
        );
    }

    #[test]
    fn cached_user_construction() {
        let user = CachedUser {
            id: Id::new(100),
            name: "testuser".to_string(),
            discriminator: None,
            display_name: Some("Test User".to_string()),
            avatar: None,
        };
        assert_eq!(user.name, "testuser");
        assert_eq!(user.display_name, Some("Test User".to_string()));
    }

    #[test]
    fn cached_role_construction() {
        let role = CachedRole {
            id: Id::new(200),
            name: "Admin".to_string(),
            color: 0xFF0000,
            position: 10,
        };
        assert_eq!(role.name, "Admin");
        assert_eq!(role.color, 0xFF0000);
    }

    #[test]
    fn read_state_construction() {
        let state = ReadState {
            last_message_id: Id::new(999),
            mention_count: 3,
        };
        assert_eq!(state.last_message_id.get(), 999);
        assert_eq!(state.mention_count, 3);
    }

    #[test]
    fn message_reference_serialization() {
        let reference = MessageReference {
            message_id: Some(Id::new(100)),
            channel_id: Some(Id::new(200)),
            guild_id: None,
        };
        let json = serde_json::to_string(&reference).unwrap();
        let deserialized: MessageReference = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message_id, Some(Id::new(100)));
        assert_eq!(deserialized.channel_id, Some(Id::new(200)));
        assert!(deserialized.guild_id.is_none());
    }

    #[test]
    fn message_attachment_serialization() {
        let attachment = MessageAttachment {
            filename: "test.txt".to_string(),
            size: 1024,
            url: "https://example.com/test.txt".to_string(),
            content_type: Some("text/plain".to_string()),
        };
        let json = serde_json::to_string(&attachment).unwrap();
        let deserialized: MessageAttachment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.filename, "test.txt");
        assert_eq!(deserialized.size, 1024);
    }

    #[test]
    fn message_embed_serialization() {
        let embed = MessageEmbed {
            title: Some("Title".to_string()),
            description: None,
            color: Some(0x00FF00),
            url: None,
        };
        let json = serde_json::to_string(&embed).unwrap();
        let deserialized: MessageEmbed = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, Some("Title".to_string()));
        assert!(deserialized.description.is_none());
        assert_eq!(deserialized.color, Some(0x00FF00));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // --- P10.1: MessageAttachment roundtrip ---
    proptest! {
        #[test]
        fn attachment_roundtrip(
            filename in "[a-zA-Z0-9._-]{1,30}",
            size in 0u64..u64::MAX,
            content_type in proptest::option::of("[a-z]+/[a-z]+"),
        ) {
            let attachment = MessageAttachment {
                filename: filename.clone(),
                size,
                url: "https://example.com/file".to_string(),
                content_type: content_type.clone(),
            };
            let json = serde_json::to_string(&attachment).unwrap();
            let restored: MessageAttachment = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(&restored.filename, &filename);
            prop_assert_eq!(restored.size, size);
            prop_assert_eq!(&restored.content_type, &content_type);
        }
    }

    // --- P10.2: MessageEmbed roundtrip ---
    proptest! {
        #[test]
        fn embed_roundtrip(
            title in proptest::option::of("[a-zA-Z0-9 ]{1,30}"),
            description in proptest::option::of("[a-zA-Z0-9 ]{1,50}"),
            color in proptest::option::of(0u32..0xFFFFFF),
        ) {
            let embed = MessageEmbed {
                title: title.clone(),
                description: description.clone(),
                color,
                url: None,
            };
            let json = serde_json::to_string(&embed).unwrap();
            let restored: MessageEmbed = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(&restored.title, &title);
            prop_assert_eq!(&restored.description, &description);
            prop_assert_eq!(restored.color, color);
        }
    }

    // --- P10.3: MessageReference roundtrip ---
    proptest! {
        #[test]
        fn message_reference_roundtrip(
            msg_id in proptest::option::of(1u64..u64::MAX),
            ch_id in proptest::option::of(1u64..u64::MAX),
            guild_id in proptest::option::of(1u64..u64::MAX),
        ) {
            let reference = MessageReference {
                message_id: msg_id.map(Id::new),
                channel_id: ch_id.map(Id::new),
                guild_id: guild_id.map(Id::new),
            };
            let json = serde_json::to_string(&reference).unwrap();
            let restored: MessageReference = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(restored.message_id.map(|id| id.get()), msg_id);
            prop_assert_eq!(restored.channel_id.map(|id| id.get()), ch_id);
            prop_assert_eq!(restored.guild_id.map(|id| id.get()), guild_id);
        }
    }

    // --- P10.4: PaneId roundtrip ---
    proptest! {
        #[test]
        fn pane_id_roundtrip(val in 0u32..u32::MAX) {
            let id = PaneId(val);
            let json = serde_json::to_string(&id).unwrap();
            let restored: PaneId = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(restored, id);
        }
    }

    // --- P10.5: SplitDirection roundtrip ---
    proptest! {
        #[test]
        fn split_direction_roundtrip(dir in prop_oneof![Just(SplitDirection::Horizontal), Just(SplitDirection::Vertical)]) {
            let json = serde_json::to_string(&dir).unwrap();
            let restored: SplitDirection = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(restored, dir);
        }
    }
}

use std::collections::{HashMap, VecDeque};

use crate::domain::types::{
    CachedChannel, CachedGuild, CachedMessage, CachedRole, CachedUser, ChannelMarker, GuildMarker,
    Id, MessageMarker, ReadState, RoleMarker, UserMarker, MAX_CACHED_MESSAGES_PER_CHANNEL,
};

/// In-memory cache of Discord state. All lookups are O(1) by ID.
/// Optimized for the two hot paths: message receive and render.
#[derive(Debug, Clone, Default)]
pub struct DiscordCache {
    /// O(1) lookup by ID
    pub guilds: HashMap<Id<GuildMarker>, CachedGuild>,
    pub channels: HashMap<Id<ChannelMarker>, CachedChannel>,
    pub users: HashMap<Id<UserMarker>, CachedUser>,

    /// Ordered guild list for sidebar rendering
    pub guild_order: Vec<Id<GuildMarker>>,

    /// Per-channel message windows
    pub messages: HashMap<Id<ChannelMarker>, VecDeque<CachedMessage>>,

    /// Per-channel typing indicators: (`user_id`, `started_at`)
    pub typing: HashMap<Id<ChannelMarker>, Vec<(Id<UserMarker>, std::time::Instant)>>,

    /// Channel → Guild reverse lookup
    pub channel_guild: HashMap<Id<ChannelMarker>, Id<GuildMarker>>,

    /// Unread state per channel
    pub read_states: HashMap<Id<ChannelMarker>, ReadState>,

    /// DM channels from READY event (never mutated via API)
    pub dm_channels: Vec<Id<ChannelMarker>>,
}

impl DiscordCache {
    /// Resolve a user ID to their display name (or username as fallback).
    pub fn resolve_user_name(&self, id: Id<UserMarker>) -> String {
        self.users.get(&id).map_or_else(
            || format!("Unknown({})", id.get()),
            |u| u.display_name.as_deref().unwrap_or(&u.name).to_string(),
        )
    }

    /// Resolve a channel ID to its name.
    pub fn resolve_channel_name(&self, id: Id<ChannelMarker>) -> String {
        self.channels
            .get(&id)
            .map_or_else(|| format!("Unknown({})", id.get()), |c| c.name.clone())
    }

    /// Resolve a role ID within a guild to its cached data.
    pub fn resolve_role(
        &self,
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
    ) -> Option<&CachedRole> {
        self.guilds
            .get(&guild_id)
            .and_then(|g| g.roles.get(&role_id))
    }

    /// Insert a message into the per-channel cache, evicting the oldest
    /// if the channel exceeds `MAX_CACHED_MESSAGES_PER_CHANNEL`.
    pub fn insert_message(&mut self, msg: CachedMessage) {
        let deque = self.messages.entry(msg.channel_id).or_default();
        deque.push_back(msg);
        while deque.len() > MAX_CACHED_MESSAGES_PER_CHANNEL {
            deque.pop_front();
        }
    }

    /// Prepend messages to the front of a channel's message cache (history backfill).
    /// Evicts from the front if total exceeds capacity.
    pub fn prepend_messages(
        &mut self,
        channel_id: Id<ChannelMarker>,
        messages: Vec<CachedMessage>,
    ) {
        let deque = self.messages.entry(channel_id).or_default();
        for msg in messages.into_iter().rev() {
            deque.push_front(msg);
        }
        while deque.len() > MAX_CACHED_MESSAGES_PER_CHANNEL {
            deque.pop_back();
        }
    }

    /// Update a message in the cache by ID. Returns true if found.
    pub fn update_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: String,
        edited_timestamp: Option<String>,
    ) -> bool {
        if let Some(deque) = self.messages.get_mut(&channel_id) {
            if let Some(msg) = deque.iter_mut().find(|m| m.id == message_id) {
                msg.content = content;
                msg.edited_timestamp = edited_timestamp;
                msg.rendered = None; // Invalidate cached render
                return true;
            }
        }
        false
    }

    /// Delete a message from the cache by ID. Returns true if found.
    pub fn delete_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> bool {
        if let Some(deque) = self.messages.get_mut(&channel_id) {
            if let Some(pos) = deque.iter().position(|m| m.id == message_id) {
                deque.remove(pos);
                return true;
            }
        }
        false
    }

    /// Add a guild to the cache and update `guild_order`.
    pub fn insert_guild(&mut self, guild: CachedGuild) {
        let id = guild.id;
        // Remove stale channel_guild entries from old guild data
        if let Some(old_guild) = self.guilds.get(&id) {
            for &old_ch_id in &old_guild.channel_order {
                if !guild.channel_order.contains(&old_ch_id) {
                    self.channel_guild.remove(&old_ch_id);
                }
            }
        }
        // Update channel_guild reverse lookup for all channels
        for &ch_id in &guild.channel_order {
            self.channel_guild.insert(ch_id, id);
        }
        self.guilds.insert(id, guild);
        if !self.guild_order.contains(&id) {
            self.guild_order.push(id);
        }
    }

    /// Add a channel to the cache and update reverse lookup.
    pub fn insert_channel(&mut self, channel: CachedChannel) {
        let id = channel.id;
        if let Some(guild_id) = channel.guild_id {
            self.channel_guild.insert(id, guild_id);
            // Add to guild's channel_order if not present
            if let Some(guild) = self.guilds.get_mut(&guild_id) {
                if !guild.channel_order.contains(&id) {
                    guild.channel_order.push(id);
                }
            }
        }
        self.channels.insert(id, channel);
    }

    /// Remove a channel from the cache.
    pub fn remove_channel(&mut self, channel_id: Id<ChannelMarker>) {
        self.channels.remove(&channel_id);
        if let Some(guild_id) = self.channel_guild.remove(&channel_id) {
            if let Some(guild) = self.guilds.get_mut(&guild_id) {
                guild.channel_order.retain(|&id| id != channel_id);
            }
        }
        self.messages.remove(&channel_id);
        self.typing.remove(&channel_id);
        self.read_states.remove(&channel_id);
    }

    /// Remove a guild from the cache.
    pub fn remove_guild(&mut self, guild_id: Id<GuildMarker>) {
        if let Some(guild) = self.guilds.remove(&guild_id) {
            for ch_id in &guild.channel_order {
                self.channels.remove(ch_id);
                self.channel_guild.remove(ch_id);
                self.messages.remove(ch_id);
                self.typing.remove(ch_id);
                self.read_states.remove(ch_id);
            }
        }
        self.guild_order.retain(|&id| id != guild_id);
    }

    /// Get all pane-relevant channel IDs that are viewing a specific channel.
    /// Useful for routing gateway events to affected panes.
    pub fn get_messages(&self, channel_id: Id<ChannelMarker>) -> Option<&VecDeque<CachedMessage>> {
        self.messages.get(&channel_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_message(id: u64, channel_id: u64, content: &str) -> CachedMessage {
        CachedMessage {
            id: Id::new(id),
            channel_id: Id::new(channel_id),
            author_id: Id::new(100),
            content: content.to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        }
    }

    fn make_guild(id: u64, name: &str, channels: Vec<u64>) -> CachedGuild {
        CachedGuild {
            id: Id::new(id),
            name: name.to_string(),
            icon: None,
            channel_order: channels.into_iter().map(Id::new).collect(),
            roles: HashMap::new(),
        }
    }

    fn make_channel(id: u64, guild_id: Option<u64>, name: &str) -> CachedChannel {
        CachedChannel {
            id: Id::new(id),
            guild_id: guild_id.map(Id::new),
            name: name.to_string(),
            kind: twilight_model::channel::ChannelType::GuildText,
            position: 0,
            parent_id: None,
            topic: None,
        }
    }

    fn make_user(id: u64, name: &str, display: Option<&str>) -> CachedUser {
        CachedUser {
            id: Id::new(id),
            name: name.to_string(),
            discriminator: None,
            display_name: display.map(String::from),
            avatar: None,
        }
    }

    // --- resolve_user_name ---

    #[test]
    fn resolve_user_name_uses_display_name() {
        let mut cache = DiscordCache::default();
        cache
            .users
            .insert(Id::new(1), make_user(1, "user1", Some("Display")));
        assert_eq!(cache.resolve_user_name(Id::new(1)), "Display");
    }

    #[test]
    fn resolve_user_name_falls_back_to_username() {
        let mut cache = DiscordCache::default();
        cache.users.insert(Id::new(1), make_user(1, "user1", None));
        assert_eq!(cache.resolve_user_name(Id::new(1)), "user1");
    }

    #[test]
    fn resolve_user_name_unknown() {
        let cache = DiscordCache::default();
        let name = cache.resolve_user_name(Id::new(999));
        assert!(name.contains("Unknown"));
    }

    // --- resolve_channel_name ---

    #[test]
    fn resolve_channel_name_found() {
        let mut cache = DiscordCache::default();
        cache
            .channels
            .insert(Id::new(10), make_channel(10, Some(1), "general"));
        assert_eq!(cache.resolve_channel_name(Id::new(10)), "general");
    }

    #[test]
    fn resolve_channel_name_unknown() {
        let cache = DiscordCache::default();
        let name = cache.resolve_channel_name(Id::new(999));
        assert!(name.contains("Unknown"));
    }

    // --- resolve_role ---

    #[test]
    fn resolve_role_found() {
        let mut cache = DiscordCache::default();
        let mut roles = HashMap::new();
        roles.insert(
            Id::new(200),
            CachedRole {
                id: Id::new(200),
                name: "Admin".to_string(),
                color: 0xFF0000,
                position: 10,
            },
        );
        cache.guilds.insert(
            Id::new(1),
            CachedGuild {
                id: Id::new(1),
                name: "Test".to_string(),
                icon: None,
                channel_order: vec![],
                roles,
            },
        );
        let role = cache.resolve_role(Id::new(1), Id::new(200));
        assert!(role.is_some());
        assert_eq!(role.unwrap().name, "Admin");
        assert_eq!(role.unwrap().color, 0xFF0000);
    }

    #[test]
    fn resolve_role_not_found() {
        let cache = DiscordCache::default();
        assert!(cache.resolve_role(Id::new(1), Id::new(200)).is_none());
    }

    #[test]
    fn resolve_role_wrong_guild() {
        let mut cache = DiscordCache::default();
        let mut roles = HashMap::new();
        roles.insert(
            Id::new(200),
            CachedRole {
                id: Id::new(200),
                name: "Admin".to_string(),
                color: 0xFF0000,
                position: 10,
            },
        );
        cache.guilds.insert(
            Id::new(1),
            CachedGuild {
                id: Id::new(1),
                name: "Test".to_string(),
                icon: None,
                channel_order: vec![],
                roles,
            },
        );
        // Different guild ID
        assert!(cache.resolve_role(Id::new(2), Id::new(200)).is_none());
    }

    // --- insert_message + eviction ---

    #[test]
    fn insert_message_adds_to_channel() {
        let mut cache = DiscordCache::default();
        cache.insert_message(make_message(1, 10, "Hello"));
        assert_eq!(cache.messages.get(&Id::new(10)).unwrap().len(), 1);
    }

    #[test]
    fn insert_message_multiple_channels() {
        let mut cache = DiscordCache::default();
        cache.insert_message(make_message(1, 10, "Chan 10"));
        cache.insert_message(make_message(2, 20, "Chan 20"));
        cache.insert_message(make_message(3, 10, "Chan 10 again"));

        assert_eq!(cache.messages.get(&Id::new(10)).unwrap().len(), 2);
        assert_eq!(cache.messages.get(&Id::new(20)).unwrap().len(), 1);
    }

    #[test]
    fn insert_message_evicts_at_capacity() {
        let mut cache = DiscordCache::default();
        // Fill to capacity
        for i in 1..=(MAX_CACHED_MESSAGES_PER_CHANNEL as u64) {
            cache.insert_message(make_message(i, 10, &format!("msg {}", i)));
        }
        assert_eq!(
            cache.messages.get(&Id::new(10)).unwrap().len(),
            MAX_CACHED_MESSAGES_PER_CHANNEL
        );

        // One more should evict the oldest
        cache.insert_message(make_message(999, 10, "overflow"));
        let deque = cache.messages.get(&Id::new(10)).unwrap();
        assert_eq!(deque.len(), MAX_CACHED_MESSAGES_PER_CHANNEL);

        // First message should be evicted, second should remain
        assert_eq!(deque.front().unwrap().id, Id::new(2));
        assert_eq!(deque.back().unwrap().content, "overflow");
    }

    // --- prepend_messages ---

    #[test]
    fn prepend_messages_adds_to_front() {
        let mut cache = DiscordCache::default();
        cache.insert_message(make_message(10, 10, "existing"));

        let history = vec![make_message(1, 10, "old1"), make_message(2, 10, "old2")];
        cache.prepend_messages(Id::new(10), history);

        let deque = cache.messages.get(&Id::new(10)).unwrap();
        assert_eq!(deque.len(), 3);
        assert_eq!(deque[0].content, "old1");
        assert_eq!(deque[1].content, "old2");
        assert_eq!(deque[2].content, "existing");
    }

    #[test]
    fn prepend_messages_evicts_from_back() {
        let mut cache = DiscordCache::default();
        // Fill to capacity
        for i in 1..=(MAX_CACHED_MESSAGES_PER_CHANNEL as u64) {
            cache.insert_message(make_message(i, 10, &format!("msg {}", i)));
        }

        // Prepend more - should evict from back (newest)
        let history = vec![make_message(500, 10, "old history")];
        cache.prepend_messages(Id::new(10), history);

        let deque = cache.messages.get(&Id::new(10)).unwrap();
        assert_eq!(deque.len(), MAX_CACHED_MESSAGES_PER_CHANNEL);
        assert_eq!(deque.front().unwrap().content, "old history");
    }

    // --- update_message ---

    #[test]
    fn update_message_changes_content() {
        let mut cache = DiscordCache::default();
        cache.insert_message(make_message(1, 10, "original"));

        let found = cache.update_message(
            Id::new(10),
            Id::new(1),
            "updated".to_string(),
            Some("2024-01-01T01:00:00Z".to_string()),
        );
        assert!(found);

        let msg = &cache.messages.get(&Id::new(10)).unwrap()[0];
        assert_eq!(msg.content, "updated");
        assert_eq!(
            msg.edited_timestamp,
            Some("2024-01-01T01:00:00Z".to_string())
        );
        assert!(msg.rendered.is_none()); // Render cache invalidated
    }

    #[test]
    fn update_message_not_found() {
        let mut cache = DiscordCache::default();
        let found = cache.update_message(Id::new(10), Id::new(999), "nope".to_string(), None);
        assert!(!found);
    }

    #[test]
    fn update_message_invalidates_rendered_cache() {
        let mut cache = DiscordCache::default();
        let mut msg = make_message(1, 10, "original");
        msg.rendered = Some(vec![ratatui::text::Line::raw("cached")]);
        cache.insert_message(msg);

        // Verify rendered is set
        assert!(cache.messages.get(&Id::new(10)).unwrap()[0]
            .rendered
            .is_some());

        cache.update_message(Id::new(10), Id::new(1), "updated".to_string(), None);

        // Rendered should be cleared
        assert!(cache.messages.get(&Id::new(10)).unwrap()[0]
            .rendered
            .is_none());
    }

    // --- delete_message ---

    #[test]
    fn delete_message_removes_from_cache() {
        let mut cache = DiscordCache::default();
        cache.insert_message(make_message(1, 10, "to delete"));
        cache.insert_message(make_message(2, 10, "to keep"));

        let found = cache.delete_message(Id::new(10), Id::new(1));
        assert!(found);

        let deque = cache.messages.get(&Id::new(10)).unwrap();
        assert_eq!(deque.len(), 1);
        assert_eq!(deque[0].content, "to keep");
    }

    #[test]
    fn delete_message_not_found() {
        let mut cache = DiscordCache::default();
        let found = cache.delete_message(Id::new(10), Id::new(999));
        assert!(!found);
    }

    // --- insert_guild ---

    #[test]
    fn insert_guild_adds_to_cache() {
        let mut cache = DiscordCache::default();
        cache.insert_guild(make_guild(1, "Test Server", vec![10, 20]));

        assert!(cache.guilds.contains_key(&Id::new(1)));
        assert!(cache.guild_order.contains(&Id::new(1)));
        assert_eq!(cache.channel_guild.get(&Id::new(10)), Some(&Id::new(1)));
        assert_eq!(cache.channel_guild.get(&Id::new(20)), Some(&Id::new(1)));
    }

    #[test]
    fn insert_guild_no_duplicate_in_order() {
        let mut cache = DiscordCache::default();
        cache.insert_guild(make_guild(1, "Test", vec![]));
        cache.insert_guild(make_guild(1, "Test Updated", vec![]));

        assert_eq!(cache.guild_order.len(), 1);
        assert_eq!(cache.guilds.get(&Id::new(1)).unwrap().name, "Test Updated");
    }

    // --- insert_channel ---

    #[test]
    fn insert_channel_adds_to_cache() {
        let mut cache = DiscordCache::default();
        cache.insert_guild(make_guild(1, "Test", vec![]));
        cache.insert_channel(make_channel(10, Some(1), "general"));

        assert!(cache.channels.contains_key(&Id::new(10)));
        assert_eq!(cache.channel_guild.get(&Id::new(10)), Some(&Id::new(1)));
        // Should be added to guild's channel_order
        assert!(cache
            .guilds
            .get(&Id::new(1))
            .unwrap()
            .channel_order
            .contains(&Id::new(10)));
    }

    #[test]
    fn insert_channel_dm_no_guild() {
        let mut cache = DiscordCache::default();
        cache.insert_channel(make_channel(10, None, "DM"));

        assert!(cache.channels.contains_key(&Id::new(10)));
        assert!(!cache.channel_guild.contains_key(&Id::new(10)));
    }

    // --- remove_channel ---

    #[test]
    fn remove_channel_cleans_up() {
        let mut cache = DiscordCache::default();
        cache.insert_guild(make_guild(1, "Test", vec![10]));
        cache.insert_channel(make_channel(10, Some(1), "general"));
        cache.insert_message(make_message(1, 10, "msg"));

        cache.remove_channel(Id::new(10));

        assert!(!cache.channels.contains_key(&Id::new(10)));
        assert!(!cache.channel_guild.contains_key(&Id::new(10)));
        assert!(!cache.messages.contains_key(&Id::new(10)));
        assert!(!cache
            .guilds
            .get(&Id::new(1))
            .unwrap()
            .channel_order
            .contains(&Id::new(10)));
    }

    // --- remove_guild ---

    #[test]
    fn remove_guild_cleans_up() {
        let mut cache = DiscordCache::default();
        cache.insert_guild(make_guild(1, "Test", vec![10, 20]));
        cache.insert_channel(make_channel(10, Some(1), "general"));
        cache.insert_channel(make_channel(20, Some(1), "random"));

        cache.remove_guild(Id::new(1));

        assert!(!cache.guilds.contains_key(&Id::new(1)));
        assert!(!cache.guild_order.contains(&Id::new(1)));
        assert!(!cache.channels.contains_key(&Id::new(10)));
        assert!(!cache.channels.contains_key(&Id::new(20)));
    }

    // --- get_messages ---

    #[test]
    fn get_messages_returns_channel_messages() {
        let mut cache = DiscordCache::default();
        cache.insert_message(make_message(1, 10, "Hello"));
        cache.insert_message(make_message(2, 10, "World"));

        let msgs = cache.get_messages(Id::new(10)).unwrap();
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn get_messages_returns_none_for_empty() {
        let cache = DiscordCache::default();
        assert!(cache.get_messages(Id::new(10)).is_none());
    }

    // --- guild_order ---

    #[test]
    fn guild_order_maintained() {
        let mut cache = DiscordCache::default();
        cache.insert_guild(make_guild(3, "C", vec![]));
        cache.insert_guild(make_guild(1, "A", vec![]));
        cache.insert_guild(make_guild(2, "B", vec![]));

        // Order follows insertion order
        assert_eq!(cache.guild_order, vec![Id::new(3), Id::new(1), Id::new(2)]);
    }

    // --- channel_guild reverse lookup ---

    #[test]
    fn channel_guild_reverse_lookup() {
        let mut cache = DiscordCache::default();
        cache.insert_guild(make_guild(1, "Server", vec![10, 20]));
        cache.insert_guild(make_guild(2, "Other", vec![30]));

        assert_eq!(cache.channel_guild.get(&Id::new(10)), Some(&Id::new(1)));
        assert_eq!(cache.channel_guild.get(&Id::new(20)), Some(&Id::new(1)));
        assert_eq!(cache.channel_guild.get(&Id::new(30)), Some(&Id::new(2)));
    }

    // --- DM channels ---

    #[test]
    fn dm_channels_stored() {
        let mut cache = DiscordCache::default();
        cache.dm_channels = vec![Id::new(100), Id::new(200)];
        assert_eq!(cache.dm_channels.len(), 2);
    }

    // --- read_states ---

    #[test]
    fn read_states_tracked() {
        let mut cache = DiscordCache::default();
        cache.read_states.insert(
            Id::new(10),
            ReadState {
                last_message_id: Id::new(999),
                mention_count: 3,
            },
        );
        let rs = cache.read_states.get(&Id::new(10)).unwrap();
        assert_eq!(rs.mention_count, 3);
        assert_eq!(rs.last_message_id.get(), 999);
    }

    // --- Default ---

    #[test]
    fn default_cache_is_empty() {
        let cache = DiscordCache::default();
        assert!(cache.guilds.is_empty());
        assert!(cache.channels.is_empty());
        assert!(cache.users.is_empty());
        assert!(cache.guild_order.is_empty());
        assert!(cache.messages.is_empty());
        assert!(cache.dm_channels.is_empty());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // --- P2.1: messages never exceed MAX_CACHED_MESSAGES_PER_CHANNEL ---
    proptest! {
        #[test]
        fn insert_message_never_exceeds_capacity(count in 1usize..500) {
            let mut cache = DiscordCache::default();
            let channel_id = Id::new(10);
            for i in 1..=count {
                cache.insert_message(CachedMessage {
                    id: Id::new(i as u64),
                    channel_id,
                    author_id: Id::new(1),
                    content: format!("msg {}", i),
                    timestamp: "2024-01-01T00:00:00Z".to_string(),
                    edited_timestamp: None,
                    attachments: vec![],
                    embeds: vec![],
                    message_reference: None,
                    mention_everyone: false,
                    mentions: vec![],
                    rendered: None,
                });
            }
            let len = cache.messages.get(&channel_id).map(|d| d.len()).unwrap_or(0);
            prop_assert!(len <= MAX_CACHED_MESSAGES_PER_CHANNEL,
                "Messages {} > max {}", len, MAX_CACHED_MESSAGES_PER_CHANNEL);
        }
    }

    // --- P2.2: insert_guild idempotent for guild_order ---
    proptest! {
        #[test]
        fn insert_guild_no_duplicate_order(n_inserts in 1usize..10) {
            let mut cache = DiscordCache::default();
            let guild = CachedGuild {
                id: Id::new(42),
                name: "Test".to_string(),
                icon: None,
                channel_order: vec![],
                roles: std::collections::HashMap::new(),
            };
            for _ in 0..n_inserts {
                cache.insert_guild(guild.clone());
            }
            let count = cache.guild_order.iter().filter(|&&id| id == Id::new(42)).count();
            prop_assert_eq!(count, 1, "Guild appeared {} times in guild_order", count);
        }
    }

    // --- P2.3: channel_guild reverse lookup consistent after insert_guild ---
    proptest! {
        #[test]
        fn channel_guild_consistent_after_insert(
            ch_ids in proptest::collection::vec(1u64..1000, 1..10)
        ) {
            let mut cache = DiscordCache::default();
            let guild_id = Id::new(1);
            let channel_order: Vec<Id<ChannelMarker>> = ch_ids.iter().map(|&id| Id::new(id)).collect();
            let guild = CachedGuild {
                id: guild_id,
                name: "G".to_string(),
                icon: None,
                channel_order: channel_order.clone(),
                roles: std::collections::HashMap::new(),
            };
            cache.insert_guild(guild);
            for ch_id in &channel_order {
                prop_assert_eq!(
                    cache.channel_guild.get(ch_id),
                    Some(&guild_id),
                    "channel_guild missing entry for channel {:?}", ch_id
                );
            }
        }
    }

    // --- P2.4: remove_guild clears all channel_guild entries ---
    proptest! {
        #[test]
        fn remove_guild_clears_channel_guild(
            ch_ids in proptest::collection::vec(1u64..1000, 1..10)
        ) {
            let mut cache = DiscordCache::default();
            let guild_id = Id::new(1);
            let channel_order: Vec<Id<ChannelMarker>> = ch_ids.iter().map(|&id| Id::new(id)).collect();
            let guild = CachedGuild {
                id: guild_id,
                name: "G".to_string(),
                icon: None,
                channel_order: channel_order.clone(),
                roles: std::collections::HashMap::new(),
            };
            cache.insert_guild(guild);
            // Also insert the channels themselves
            for &ch_id in &channel_order {
                cache.channels.insert(ch_id, CachedChannel {
                    id: ch_id,
                    guild_id: Some(guild_id),
                    name: "ch".to_string(),
                    kind: twilight_model::channel::ChannelType::GuildText,
                    position: 0,
                    parent_id: None,
                    topic: None,
                });
            }
            cache.remove_guild(guild_id);
            for ch_id in &channel_order {
                prop_assert!(
                    !cache.channel_guild.contains_key(ch_id),
                    "channel_guild still has entry for {:?} after guild removal", ch_id
                );
                prop_assert!(
                    !cache.channels.contains_key(ch_id),
                    "channels still has entry for {:?} after guild removal", ch_id
                );
            }
            prop_assert!(!cache.guilds.contains_key(&guild_id));
        }
    }

    // --- P2.5: remove_channel clears associated data ---
    proptest! {
        #[test]
        fn remove_channel_clears_data(n_messages in 0usize..50) {
            let mut cache = DiscordCache::default();
            let guild_id = Id::new(1);
            let ch_id = Id::new(10);
            cache.insert_guild(CachedGuild {
                id: guild_id,
                name: "G".to_string(),
                icon: None,
                channel_order: vec![ch_id],
                roles: std::collections::HashMap::new(),
            });
            cache.insert_channel(CachedChannel {
                id: ch_id,
                guild_id: Some(guild_id),
                name: "ch".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 0,
                parent_id: None,
                topic: None,
            });
            for i in 1..=n_messages {
                cache.insert_message(CachedMessage {
                    id: Id::new(i as u64),
                    channel_id: ch_id,
                    author_id: Id::new(1),
                    content: "msg".to_string(),
                    timestamp: "2024-01-01T00:00:00Z".to_string(),
                    edited_timestamp: None,
                    attachments: vec![],
                    embeds: vec![],
                    message_reference: None,
                    mention_everyone: false,
                    mentions: vec![],
                    rendered: None,
                });
            }
            cache.typing.insert(ch_id, vec![]);
            cache.read_states.insert(ch_id, ReadState {
                last_message_id: Id::new(1),
                mention_count: 0,
            });

            cache.remove_channel(ch_id);
            prop_assert!(!cache.channels.contains_key(&ch_id));
            prop_assert!(!cache.messages.contains_key(&ch_id));
            prop_assert!(!cache.typing.contains_key(&ch_id));
            prop_assert!(!cache.read_states.contains_key(&ch_id));
            prop_assert!(!cache.channel_guild.contains_key(&ch_id));
        }
    }
}

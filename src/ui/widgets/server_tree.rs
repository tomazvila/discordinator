use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Widget,
};

use crate::app::{AppState, SidebarState};
use crate::domain::cache::DiscordCache;
use crate::domain::types::{ChannelMarker, GuildMarker, Id};
use crate::ui::theme::Theme;

/// A flattened item in the server/channel tree for rendering.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeItem {
    Guild {
        id: Id<GuildMarker>,
        name: String,
        collapsed: bool,
    },
    Channel {
        id: Id<ChannelMarker>,
        guild_id: Id<GuildMarker>,
        name: String,
        is_category: bool,
        indent: u16,
        unread: bool,
        mention_count: u32,
    },
    DmHeader,
    DmChannel {
        id: Id<ChannelMarker>,
        name: String,
    },
}

/// Check if a channel type is displayable in the TUI (text-based channels only).
fn is_displayable_channel(kind: twilight_model::channel::ChannelType) -> bool {
    matches!(
        kind,
        twilight_model::channel::ChannelType::GuildText
            | twilight_model::channel::ChannelType::GuildAnnouncement
            | twilight_model::channel::ChannelType::GuildForum
    )
}

/// Build the flat tree of items from cache state.
/// Channels are sorted by Discord's hierarchy: categories sorted by position,
/// with child channels nested underneath sorted by position. Orphan channels
/// (no parent category) appear before categories. Voice/stage channels are filtered out.
pub fn build_tree(cache: &DiscordCache, sidebar: &SidebarState) -> Vec<TreeItem> {
    let mut items = Vec::new();

    // Guilds
    for &guild_id in &cache.guild_order {
        if let Some(guild) = cache.guilds.get(&guild_id) {
            let collapsed = sidebar.collapsed_guilds.contains(&guild_id);
            items.push(TreeItem::Guild {
                id: guild_id,
                name: guild.name.clone(),
                collapsed,
            });

            if !collapsed {
                // Collect categories and sort by position
                let mut categories: Vec<_> = guild
                    .channel_order
                    .iter()
                    .filter_map(|&ch_id| cache.channels.get(&ch_id))
                    .filter(|ch| ch.kind == twilight_model::channel::ChannelType::GuildCategory)
                    .collect();
                categories.sort_by_key(|ch| ch.position);

                // Collect all non-category channels for this guild
                let guild_channels: Vec<_> = guild
                    .channel_order
                    .iter()
                    .filter_map(|&ch_id| cache.channels.get(&ch_id))
                    .filter(|ch| ch.kind != twilight_model::channel::ChannelType::GuildCategory)
                    .filter(|ch| is_displayable_channel(ch.kind))
                    .collect();

                // Orphan channels: no parent_id, sorted by position
                let mut orphans: Vec<_> = guild_channels
                    .iter()
                    .filter(|ch| ch.parent_id.is_none())
                    .collect();
                orphans.sort_by_key(|ch| ch.position);

                // Emit orphans first (before any categories)
                for ch in &orphans {
                    push_channel_item(&mut items, ch, guild_id, cache, false);
                }

                // Emit each category followed by its children
                for cat in &categories {
                    push_channel_item(&mut items, cat, guild_id, cache, true);

                    let mut children: Vec<_> = guild_channels
                        .iter()
                        .filter(|ch| ch.parent_id == Some(cat.id))
                        .collect();
                    children.sort_by_key(|ch| ch.position);

                    for ch in &children {
                        push_channel_item(&mut items, ch, guild_id, cache, false);
                    }
                }
            }
        }
    }

    // DM channels
    if !cache.dm_channels.is_empty() {
        items.push(TreeItem::DmHeader);
        for &channel_id in &cache.dm_channels {
            let name = cache.resolve_channel_name(channel_id);
            items.push(TreeItem::DmChannel {
                id: channel_id,
                name,
            });
        }
    }

    items
}

/// Helper to push a channel `TreeItem` into the items list.
fn push_channel_item(
    items: &mut Vec<TreeItem>,
    channel: &crate::domain::types::CachedChannel,
    guild_id: Id<GuildMarker>,
    cache: &DiscordCache,
    is_category: bool,
) {
    let indent = if is_category { 1 } else { 2 };
    let mention_count = cache
        .read_states
        .get(&channel.id)
        .map_or(0, |rs| rs.mention_count);
    let has_read_state = cache.read_states.contains_key(&channel.id);

    items.push(TreeItem::Channel {
        id: channel.id,
        guild_id,
        name: channel.name.clone(),
        is_category,
        indent,
        unread: has_read_state,
        mention_count,
    });
}

/// Server/channel tree sidebar widget.
pub struct ServerTree<'a> {
    items: Vec<TreeItem>,
    selected_index: usize,
    theme: &'a Theme,
    active_channel: Option<Id<ChannelMarker>>,
}

impl<'a> ServerTree<'a> {
    pub fn new(state: &'a AppState) -> Self {
        let items = build_tree(&state.cache, &state.sidebar);
        Self {
            items,
            selected_index: state.sidebar.selected_index,
            theme: &state.theme,
            active_channel: state.focused_pane().channel_id,
        }
    }

    pub fn from_parts(
        items: Vec<TreeItem>,
        selected_index: usize,
        theme: &'a Theme,
        active_channel: Option<Id<ChannelMarker>>,
    ) -> Self {
        Self {
            items,
            selected_index,
            theme,
            active_channel,
        }
    }
}

impl Widget for ServerTree<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let style = self.theme.sidebar_style();

        // Fill background
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_char(' ').set_style(style);
            }
        }

        // Auto-scroll: compute scroll offset so selected_index stays visible
        let visible_rows = area.height as usize;
        let scroll_offset = if visible_rows == 0 {
            0
        } else if self.selected_index >= visible_rows {
            self.selected_index - visible_rows + 1
        } else {
            0
        };

        for (i, item) in self.items.iter().enumerate().skip(scroll_offset) {
            let y = area.y + (i - scroll_offset) as u16;
            if y >= area.bottom() {
                break;
            }

            let is_selected = i == self.selected_index;

            match item {
                TreeItem::Guild {
                    name, collapsed, ..
                } => {
                    let prefix = if *collapsed { "▸ " } else { "▾ " };
                    let text = format!("{prefix}{name}");
                    let item_style = if is_selected {
                        self.theme.sidebar_selected_style()
                    } else {
                        ratatui::style::Style::default()
                            .fg(self.theme.sidebar_fg)
                            .add_modifier(Modifier::BOLD)
                    };
                    let line = Line::from(Span::styled(text, item_style));
                    buf.set_line(area.x, y, &line, area.width);
                }
                TreeItem::Channel {
                    id,
                    name,
                    is_category,
                    indent,
                    unread,
                    mention_count,
                    ..
                } => {
                    let indent_str = " ".repeat(*indent as usize);
                    let prefix = if *is_category { "" } else { "# " };
                    let is_active = self.active_channel == Some(*id);

                    let item_style = if is_selected {
                        self.theme.sidebar_selected_style()
                    } else if *mention_count > 0 {
                        ratatui::style::Style::default().fg(self.theme.sidebar_mention_fg)
                    } else if *unread || is_active {
                        ratatui::style::Style::default()
                            .fg(self.theme.sidebar_unread_fg)
                            .add_modifier(Modifier::BOLD)
                    } else if *is_category {
                        ratatui::style::Style::default().fg(self.theme.sidebar_category_fg)
                    } else {
                        style
                    };

                    let mut text = format!("{indent_str}{prefix}{name}");
                    if *mention_count > 0 {
                        use std::fmt::Write;
                        let _ = write!(text, " ({mention_count})");
                    }

                    let line = Line::from(Span::styled(text, item_style));
                    buf.set_line(area.x, y, &line, area.width);
                }
                TreeItem::DmHeader => {
                    let item_style = if is_selected {
                        self.theme.sidebar_selected_style()
                    } else {
                        ratatui::style::Style::default()
                            .fg(self.theme.sidebar_category_fg)
                            .add_modifier(Modifier::BOLD)
                    };
                    let line = Line::from(Span::styled("Direct Messages", item_style));
                    buf.set_line(area.x, y, &line, area.width);
                }
                TreeItem::DmChannel { name, .. } => {
                    let item_style = if is_selected {
                        self.theme.sidebar_selected_style()
                    } else {
                        style
                    };
                    let line = Line::from(Span::styled(format!("  {name}"), item_style));
                    buf.set_line(area.x, y, &line, area.width);
                }
            }
        }
    }
}

/// Find the tree index of a channel by its ID.
pub fn find_channel_index(items: &[TreeItem], channel_id: Id<ChannelMarker>) -> Option<usize> {
    items.iter().position(|item| match item {
        TreeItem::Channel { id, .. } | TreeItem::DmChannel { id, .. } => *id == channel_id,
        _ => false,
    })
}

/// Find the parent guild index for the item at the given index.
pub fn find_parent_guild_index(items: &[TreeItem], index: usize) -> Option<usize> {
    match items.get(index)? {
        TreeItem::Guild { .. } => Some(index),
        TreeItem::Channel { guild_id, .. } => items
            .iter()
            .position(|item| matches!(item, TreeItem::Guild { id, .. } if *id == *guild_id)),
        _ => None,
    }
}

/// Get the channel ID at the given tree index, if any.
pub fn channel_at_index(items: &[TreeItem], index: usize) -> Option<Id<ChannelMarker>> {
    items.get(index).and_then(|item| match item {
        TreeItem::Channel { id, .. } | TreeItem::DmChannel { id, .. } => Some(*id),
        _ => None,
    })
}

/// Navigate the sidebar selection up.
pub fn navigate_up(sidebar: &mut SidebarState, item_count: usize) {
    if item_count > 0 && sidebar.selected_index > 0 {
        sidebar.selected_index -= 1;
    }
}

/// Navigate the sidebar selection down.
pub fn navigate_down(sidebar: &mut SidebarState, item_count: usize) {
    if item_count > 0 && sidebar.selected_index < item_count - 1 {
        sidebar.selected_index += 1;
    }
}

/// Toggle collapse on the guild at the current selection.
pub fn toggle_collapse(sidebar: &mut SidebarState, items: &[TreeItem]) {
    if let Some(TreeItem::Guild { id, .. }) = items.get(sidebar.selected_index) {
        if sidebar.collapsed_guilds.contains(id) {
            sidebar.collapsed_guilds.remove(id);
        } else {
            sidebar.collapsed_guilds.insert(*id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{CachedChannel, CachedGuild, ReadState};
    use std::collections::HashMap;

    fn setup_cache() -> (DiscordCache, SidebarState) {
        let mut cache = DiscordCache::default();

        let guild_id = Id::new(1);
        let cat_id = Id::new(10);
        let general_id = Id::new(11);
        let random_id = Id::new(12);

        cache.guilds.insert(
            guild_id,
            CachedGuild {
                id: guild_id,
                name: "Test Server".to_string(),
                icon: None,
                channel_order: vec![cat_id, general_id, random_id],
                roles: HashMap::new(),
            },
        );
        cache.channels.insert(
            cat_id,
            CachedChannel {
                id: cat_id,
                guild_id: Some(guild_id),
                name: "Text Channels".to_string(),
                kind: twilight_model::channel::ChannelType::GuildCategory,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        cache.channels.insert(
            general_id,
            CachedChannel {
                id: general_id,
                guild_id: Some(guild_id),
                name: "general".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 1,
                parent_id: Some(cat_id),
                topic: None,
            },
        );
        cache.channels.insert(
            random_id,
            CachedChannel {
                id: random_id,
                guild_id: Some(guild_id),
                name: "random".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 2,
                parent_id: Some(cat_id),
                topic: None,
            },
        );
        cache.guild_order.push(guild_id);

        let sidebar = SidebarState::default();
        (cache, sidebar)
    }

    #[test]
    fn build_tree_with_guild_and_channels() {
        let (cache, sidebar) = setup_cache();
        let items = build_tree(&cache, &sidebar);

        // Should have: Guild + category + general + random = 4 items
        assert_eq!(items.len(), 4);

        assert!(matches!(&items[0], TreeItem::Guild { name, .. } if name == "Test Server"));
        assert!(
            matches!(&items[1], TreeItem::Channel { name, is_category: true, .. } if name == "Text Channels")
        );
        assert!(
            matches!(&items[2], TreeItem::Channel { name, is_category: false, .. } if name == "general")
        );
        assert!(
            matches!(&items[3], TreeItem::Channel { name, is_category: false, .. } if name == "random")
        );
    }

    #[test]
    fn collapsed_guild_hides_channels() {
        let (cache, mut sidebar) = setup_cache();
        sidebar.collapsed_guilds.insert(Id::new(1));

        let items = build_tree(&cache, &sidebar);
        assert_eq!(items.len(), 1); // Only the guild header
        assert!(matches!(
            &items[0],
            TreeItem::Guild {
                collapsed: true,
                ..
            }
        ));
    }

    #[test]
    fn dm_channels_appear_in_tree() {
        let mut cache = DiscordCache::default();
        let dm_id = Id::new(100);
        cache.dm_channels.push(dm_id);
        cache.channels.insert(
            dm_id,
            CachedChannel {
                id: dm_id,
                guild_id: None,
                name: "friend".to_string(),
                kind: twilight_model::channel::ChannelType::Private,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );

        let sidebar = SidebarState::default();
        let items = build_tree(&cache, &sidebar);

        assert_eq!(items.len(), 2); // DM header + DM channel
        assert!(matches!(&items[0], TreeItem::DmHeader));
        assert!(matches!(&items[1], TreeItem::DmChannel { name, .. } if name == "friend"));
    }

    #[test]
    fn navigate_up_and_down() {
        let mut sidebar = SidebarState::default();
        let item_count = 5;

        navigate_down(&mut sidebar, item_count);
        assert_eq!(sidebar.selected_index, 1);

        navigate_down(&mut sidebar, item_count);
        assert_eq!(sidebar.selected_index, 2);

        navigate_up(&mut sidebar, item_count);
        assert_eq!(sidebar.selected_index, 1);

        navigate_up(&mut sidebar, item_count);
        assert_eq!(sidebar.selected_index, 0);

        // Can't go below 0
        navigate_up(&mut sidebar, item_count);
        assert_eq!(sidebar.selected_index, 0);
    }

    #[test]
    fn navigate_down_stops_at_last() {
        let mut sidebar = SidebarState::default();
        sidebar.selected_index = 3;
        navigate_down(&mut sidebar, 4);
        assert_eq!(sidebar.selected_index, 3); // can't go past last
    }

    #[test]
    fn toggle_collapse_guild() {
        let (cache, mut sidebar) = setup_cache();
        let items = build_tree(&cache, &sidebar);

        // Select guild at index 0
        sidebar.selected_index = 0;
        toggle_collapse(&mut sidebar, &items);
        assert!(sidebar.collapsed_guilds.contains(&Id::new(1)));

        // Toggle again
        let items = build_tree(&cache, &sidebar);
        toggle_collapse(&mut sidebar, &items);
        assert!(!sidebar.collapsed_guilds.contains(&Id::new(1)));
    }

    #[test]
    fn toggle_collapse_on_channel_is_noop() {
        let (cache, mut sidebar) = setup_cache();
        let items = build_tree(&cache, &sidebar);

        sidebar.selected_index = 2; // a channel, not a guild
        toggle_collapse(&mut sidebar, &items);
        assert!(sidebar.collapsed_guilds.is_empty());
    }

    #[test]
    fn channel_at_index_returns_channel() {
        let (cache, sidebar) = setup_cache();
        let items = build_tree(&cache, &sidebar);

        assert!(channel_at_index(&items, 0).is_none()); // Guild
        assert!(channel_at_index(&items, 1).is_some()); // Category channel
        assert_eq!(channel_at_index(&items, 2), Some(Id::new(11))); // general
        assert_eq!(channel_at_index(&items, 3), Some(Id::new(12))); // random
    }

    #[test]
    fn render_tree_basic() {
        let (cache, sidebar) = setup_cache();
        let items = build_tree(&cache, &sidebar);
        let theme = Theme::default();
        let widget = ServerTree::from_parts(items, 0, &theme, None);

        let area = Rect::new(0, 0, 30, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        // Check first line has guild name
        let line0: String = (0..30)
            .map(|x| buf[(x, 0u16)].symbol().to_string())
            .collect::<String>();
        assert!(line0.contains("Test Server"), "line0 was: {}", line0);
    }

    #[test]
    fn render_tree_shows_channels() {
        let (cache, sidebar) = setup_cache();
        let items = build_tree(&cache, &sidebar);
        let theme = Theme::default();
        let widget = ServerTree::from_parts(items, 0, &theme, None);

        let area = Rect::new(0, 0, 30, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let line2: String = (0..30)
            .map(|x| buf[(x, 2u16)].symbol().to_string())
            .collect::<String>();
        assert!(line2.contains("general"), "line2 was: {}", line2);
    }

    #[test]
    fn find_channel_index_finds_channel() {
        let (cache, sidebar) = setup_cache();
        let items = build_tree(&cache, &sidebar);

        assert_eq!(find_channel_index(&items, Id::new(11)), Some(2)); // general
        assert_eq!(find_channel_index(&items, Id::new(12)), Some(3)); // random
        assert_eq!(find_channel_index(&items, Id::new(999)), None); // not found
    }

    #[test]
    fn find_channel_index_finds_dm() {
        let mut cache = DiscordCache::default();
        let dm_id = Id::new(100);
        cache.dm_channels.push(dm_id);
        cache.channels.insert(
            dm_id,
            CachedChannel {
                id: dm_id,
                guild_id: None,
                name: "friend".to_string(),
                kind: twilight_model::channel::ChannelType::Private,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        let sidebar = SidebarState::default();
        let items = build_tree(&cache, &sidebar);

        assert_eq!(find_channel_index(&items, dm_id), Some(1));
    }

    #[test]
    fn find_parent_guild_index_from_channel() {
        let (cache, sidebar) = setup_cache();
        let items = build_tree(&cache, &sidebar);

        // Channel at index 2 (general) -> parent guild at index 0
        assert_eq!(find_parent_guild_index(&items, 2), Some(0));
        // Guild at index 0 -> itself
        assert_eq!(find_parent_guild_index(&items, 0), Some(0));
        // DM items have no parent guild
        assert_eq!(find_parent_guild_index(&items, 99), None);
    }

    #[test]
    fn render_tree_scrolls_when_selection_past_area() {
        let (cache, mut sidebar) = setup_cache();
        // Select item 3 (random) but area only fits 2 rows
        sidebar.selected_index = 3;
        let items = build_tree(&cache, &sidebar);
        let theme = Theme::default();
        let widget = ServerTree::from_parts(items, 3, &theme, None);

        let area = Rect::new(0, 0, 30, 2);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        // With 2 visible rows and selected_index=3, scroll_offset=2
        // So row 0 shows items[2]="general", row 1 shows items[3]="random"
        let line1: String = (0..30)
            .map(|x| buf[(x, 1u16)].symbol().to_string())
            .collect::<String>();
        assert!(line1.contains("random"), "line1 was: {}", line1);
    }

    #[test]
    fn build_tree_sorts_channels_by_position_under_categories() {
        let mut cache = DiscordCache::default();
        let guild_id = Id::new(1);
        let cat_general = Id::new(10); // "General" category, position 0
        let cat_events = Id::new(20); // "Events" category, position 1
        let ch_hangout = Id::new(11); // under General, position 1
        let ch_rules = Id::new(12); // under General, position 0
        let ch_announcements = Id::new(21); // under Events, position 0

        cache.guilds.insert(
            guild_id,
            CachedGuild {
                id: guild_id,
                name: "Test".to_string(),
                icon: None,
                // Deliberately out of order to test sorting
                channel_order: vec![
                    ch_hangout,
                    cat_events,
                    ch_announcements,
                    ch_rules,
                    cat_general,
                ],
                roles: HashMap::new(),
            },
        );
        cache.guild_order.push(guild_id);

        // General category (position 0)
        cache.channels.insert(
            cat_general,
            CachedChannel {
                id: cat_general,
                guild_id: Some(guild_id),
                name: "General".to_string(),
                kind: twilight_model::channel::ChannelType::GuildCategory,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        // Events category (position 1)
        cache.channels.insert(
            cat_events,
            CachedChannel {
                id: cat_events,
                guild_id: Some(guild_id),
                name: "Events".to_string(),
                kind: twilight_model::channel::ChannelType::GuildCategory,
                position: 1,
                parent_id: None,
                topic: None,
            },
        );
        // rules under General (position 0)
        cache.channels.insert(
            ch_rules,
            CachedChannel {
                id: ch_rules,
                guild_id: Some(guild_id),
                name: "rules".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 0,
                parent_id: Some(cat_general),
                topic: None,
            },
        );
        // hangout under General (position 1)
        cache.channels.insert(
            ch_hangout,
            CachedChannel {
                id: ch_hangout,
                guild_id: Some(guild_id),
                name: "hangout".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 1,
                parent_id: Some(cat_general),
                topic: None,
            },
        );
        // announcements under Events (position 0)
        cache.channels.insert(
            ch_announcements,
            CachedChannel {
                id: ch_announcements,
                guild_id: Some(guild_id),
                name: "announcements".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 0,
                parent_id: Some(cat_events),
                topic: None,
            },
        );

        let sidebar = SidebarState::default();
        let items = build_tree(&cache, &sidebar);

        // Expected order: Guild, General(cat), rules, hangout, Events(cat), announcements
        assert_eq!(items.len(), 6);
        assert!(matches!(&items[0], TreeItem::Guild { name, .. } if name == "Test"));
        assert!(
            matches!(&items[1], TreeItem::Channel { name, is_category: true, .. } if name == "General")
        );
        assert!(
            matches!(&items[2], TreeItem::Channel { name, is_category: false, .. } if name == "rules")
        );
        assert!(
            matches!(&items[3], TreeItem::Channel { name, is_category: false, .. } if name == "hangout")
        );
        assert!(
            matches!(&items[4], TreeItem::Channel { name, is_category: true, .. } if name == "Events")
        );
        assert!(
            matches!(&items[5], TreeItem::Channel { name, is_category: false, .. } if name == "announcements")
        );
    }

    #[test]
    fn build_tree_orphan_channels_come_before_categories() {
        // Channels with no parent_id should appear before any categories
        let mut cache = DiscordCache::default();
        let guild_id = Id::new(1);
        let cat_id = Id::new(10);
        let orphan_id = Id::new(11);
        let child_id = Id::new(12);

        cache.guilds.insert(
            guild_id,
            CachedGuild {
                id: guild_id,
                name: "Test".to_string(),
                icon: None,
                channel_order: vec![child_id, cat_id, orphan_id],
                roles: HashMap::new(),
            },
        );
        cache.guild_order.push(guild_id);

        cache.channels.insert(
            cat_id,
            CachedChannel {
                id: cat_id,
                guild_id: Some(guild_id),
                name: "Category".to_string(),
                kind: twilight_model::channel::ChannelType::GuildCategory,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        cache.channels.insert(
            orphan_id,
            CachedChannel {
                id: orphan_id,
                guild_id: Some(guild_id),
                name: "orphan".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        cache.channels.insert(
            child_id,
            CachedChannel {
                id: child_id,
                guild_id: Some(guild_id),
                name: "child".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 0,
                parent_id: Some(cat_id),
                topic: None,
            },
        );

        let sidebar = SidebarState::default();
        let items = build_tree(&cache, &sidebar);

        // Expected: Guild, orphan (no category), Category, child
        assert_eq!(items.len(), 4);
        assert!(matches!(&items[0], TreeItem::Guild { .. }));
        assert!(
            matches!(&items[1], TreeItem::Channel { name, is_category: false, .. } if name == "orphan")
        );
        assert!(
            matches!(&items[2], TreeItem::Channel { name, is_category: true, .. } if name == "Category")
        );
        assert!(
            matches!(&items[3], TreeItem::Channel { name, is_category: false, .. } if name == "child")
        );
    }

    #[test]
    fn build_tree_filters_voice_channels() {
        let mut cache = DiscordCache::default();
        let guild_id = Id::new(1);
        let text_id = Id::new(10);
        let voice_id = Id::new(11);
        let stage_id = Id::new(12);

        cache.guilds.insert(
            guild_id,
            CachedGuild {
                id: guild_id,
                name: "Test".to_string(),
                icon: None,
                channel_order: vec![text_id, voice_id, stage_id],
                roles: HashMap::new(),
            },
        );
        cache.guild_order.push(guild_id);

        cache.channels.insert(
            text_id,
            CachedChannel {
                id: text_id,
                guild_id: Some(guild_id),
                name: "text".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        cache.channels.insert(
            voice_id,
            CachedChannel {
                id: voice_id,
                guild_id: Some(guild_id),
                name: "voice".to_string(),
                kind: twilight_model::channel::ChannelType::GuildVoice,
                position: 1,
                parent_id: None,
                topic: None,
            },
        );
        cache.channels.insert(
            stage_id,
            CachedChannel {
                id: stage_id,
                guild_id: Some(guild_id),
                name: "stage".to_string(),
                kind: twilight_model::channel::ChannelType::GuildStageVoice,
                position: 2,
                parent_id: None,
                topic: None,
            },
        );

        let sidebar = SidebarState::default();
        let items = build_tree(&cache, &sidebar);

        // Only Guild + text channel, voice/stage filtered out
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], TreeItem::Guild { .. }));
        assert!(matches!(&items[1], TreeItem::Channel { name, .. } if name == "text"));
    }

    #[test]
    fn mention_count_in_tree() {
        let (mut cache, sidebar) = setup_cache();
        cache.read_states.insert(
            Id::new(11),
            ReadState {
                last_message_id: Id::new(999),
                mention_count: 3,
            },
        );

        let items = build_tree(&cache, &sidebar);
        if let TreeItem::Channel {
            mention_count,
            unread,
            ..
        } = &items[2]
        {
            assert_eq!(*mention_count, 3);
            assert!(*unread);
        } else {
            panic!("Expected Channel at index 2");
        }
    }
}

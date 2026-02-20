use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Widget},
};

use crate::app::AppState;
use crate::ui::widgets::{server_tree::ServerTree, status_bar::StatusBar};

/// Render the full application layout into the given area.
pub fn render(area: Rect, buf: &mut Buffer, state: &AppState) {
    // Top-level vertical split: [main content | status bar (1 line)]
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // main content
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let main_area = outer[0];
    let status_area = outer[1];

    // Main content: optional sidebar | pane area
    if state.sidebar_visible {
        let sidebar_width = state.config.appearance.sidebar_width;
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_width),
                Constraint::Min(10),
            ])
            .split(main_area);

        let sidebar_area = horizontal[0];
        let pane_area = horizontal[1];

        // Render sidebar with border
        let sidebar_block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(state.theme.inactive_border_style());
        let sidebar_inner = sidebar_block.inner(sidebar_area);
        sidebar_block.render(sidebar_area, buf);

        let tree = ServerTree::new(state);
        tree.render(sidebar_inner, buf);

        crate::ui::pane_renderer::render_pane_tree(pane_area, buf, state);
    } else {
        crate::ui::pane_renderer::render_pane_tree(main_area, buf, state);
    }

    // Status bar
    let status = StatusBar::new(state);
    status.render(status_area, buf);
}

/// Calculate the layout areas for testing purposes.
pub fn calculate_layout(
    area: Rect,
    sidebar_visible: bool,
    sidebar_width: u16,
) -> LayoutAreas {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    let main_area = outer[0];
    let status_area = outer[1];

    let (sidebar_area, pane_area) = if sidebar_visible {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_width),
                Constraint::Min(10),
            ])
            .split(main_area);
        (Some(horizontal[0]), horizontal[1])
    } else {
        (None, main_area)
    };

    LayoutAreas {
        sidebar: sidebar_area,
        pane: pane_area,
        status_bar: status_area,
    }
}

/// Layout areas returned by calculate_layout for testing.
#[derive(Debug)]
pub struct LayoutAreas {
    pub sidebar: Option<Rect>,
    pub pane: Rect,
    pub status_bar: Rect,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use crate::config::AppConfig;
    use crate::domain::types::*;

    #[test]
    fn layout_with_sidebar() {
        let area = Rect::new(0, 0, 80, 24);
        let areas = calculate_layout(area, true, 24);

        assert!(areas.sidebar.is_some());
        let sidebar = areas.sidebar.unwrap();
        assert_eq!(sidebar.width, 24);
        assert_eq!(sidebar.x, 0);

        assert_eq!(areas.pane.x, 24);
        assert_eq!(areas.pane.width, 56);

        assert_eq!(areas.status_bar.height, 1);
        assert_eq!(areas.status_bar.y, 23);
        assert_eq!(areas.status_bar.width, 80);
    }

    #[test]
    fn layout_without_sidebar() {
        let area = Rect::new(0, 0, 80, 24);
        let areas = calculate_layout(area, false, 24);

        assert!(areas.sidebar.is_none());
        assert_eq!(areas.pane.x, 0);
        assert_eq!(areas.pane.width, 80);

        assert_eq!(areas.status_bar.height, 1);
    }

    #[test]
    fn status_bar_always_at_bottom() {
        let area = Rect::new(0, 0, 120, 40);
        let areas = calculate_layout(area, true, 30);
        assert_eq!(areas.status_bar.y, 39);
        assert_eq!(areas.status_bar.height, 1);
    }

    #[test]
    fn pane_area_fills_remaining_space() {
        let area = Rect::new(0, 0, 100, 30);
        let areas = calculate_layout(area, true, 20);

        assert_eq!(areas.pane.width, 80); // 100 - 20
        assert_eq!(areas.pane.height, 29); // 30 - 1 status bar
    }

    #[test]
    fn full_render_does_not_panic() {
        let state = AppState::new(AppConfig::default());
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, &state);
        // Just verify it doesn't panic — the render fills the buffer
    }

    #[test]
    fn render_with_sidebar_hidden() {
        let mut state = AppState::new(AppConfig::default());
        state.sidebar_visible = false;
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, &state);
    }

    #[test]
    fn render_with_channel_selected() {
        let mut state = AppState::new(AppConfig::default());
        let guild_id = Id::new(1);
        let channel_id = Id::new(10);
        state.cache.guilds.insert(
            guild_id,
            CachedGuild {
                id: guild_id,
                name: "Test".to_string(),
                icon: None,
                channel_order: vec![channel_id],
                roles: std::collections::HashMap::new(),
            },
        );
        state.cache.channels.insert(
            channel_id,
            CachedChannel {
                id: channel_id,
                guild_id: Some(guild_id),
                name: "general".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        state.cache.channel_guild.insert(channel_id, guild_id);
        state.focused_pane_mut().channel_id = Some(channel_id);
        state.focused_pane_mut().guild_id = Some(guild_id);

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, &state);

        // Verify the title bar shows the channel
        let mut found = false;
        for y in 0..24 {
            let line: String = (0..80)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect::<String>();
            if line.contains("general") {
                found = true;
                break;
            }
        }
        assert!(found, "Should show channel name in the pane title");
    }

    #[test]
    fn render_with_messages() {
        let mut state = AppState::new(AppConfig::default());
        let channel_id = Id::new(10);
        state.focused_pane_mut().channel_id = Some(channel_id);

        let mut messages = VecDeque::new();
        messages.push_back(CachedMessage {
            id: Id::new(1),
            channel_id,
            author_id: Id::new(100),
            content: "Hello from test".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
            rendered: None,
        });
        state.cache.messages.insert(channel_id, messages);
        state.cache.users.insert(
            Id::new(100),
            CachedUser {
                id: Id::new(100),
                name: "TestUser".to_string(),
                discriminator: None,
                display_name: Some("TestUser".to_string()),
                avatar: None,
            },
        );

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, &state);

        let mut found = false;
        for y in 0..24 {
            let line: String = (0..80)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect::<String>();
            if line.contains("Hello from test") {
                found = true;
                break;
            }
        }
        assert!(found, "Should show message content");
    }

    #[test]
    fn small_terminal_does_not_panic() {
        let state = AppState::new(AppConfig::default());
        let area = Rect::new(0, 0, 10, 5);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, &state);
    }

    #[test]
    fn render_split_panes_both_visible() {
        let mut state = AppState::new(AppConfig::default());
        state.sidebar_visible = false;

        // Set up two channels
        let ch1 = Id::new(10);
        let ch2 = Id::new(20);
        state.cache.channels.insert(ch1, CachedChannel {
            id: ch1,
            guild_id: None,
            name: "alpha".to_string(),
            kind: twilight_model::channel::ChannelType::GuildText,
            position: 0,
            parent_id: None,
            topic: None,
        });
        state.cache.channels.insert(ch2, CachedChannel {
            id: ch2,
            guild_id: None,
            name: "beta".to_string(),
            kind: twilight_model::channel::ChannelType::GuildText,
            position: 0,
            parent_id: None,
            topic: None,
        });

        // Assign channel to first pane, split, assign channel to second pane
        state.pane_manager.assign_channel(ch1, None);
        let id1 = state.pane_manager.split(SplitDirection::Vertical);
        state.pane_manager.focused_pane_id = id1;
        state.pane_manager.assign_channel(ch2, None);

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, &state);

        // Both channel names should appear in the rendered buffer
        let mut found_alpha = false;
        let mut found_beta = false;
        for y in 0..24 {
            let line: String = (0..80)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect::<String>();
            if line.contains("alpha") {
                found_alpha = true;
            }
            if line.contains("beta") {
                found_beta = true;
            }
        }
        assert!(found_alpha, "Should show first pane's channel name 'alpha'");
        assert!(found_beta, "Should show second pane's channel name 'beta'");
    }
}

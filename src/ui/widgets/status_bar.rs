use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::Widget,
};

use crate::app::AppState;
use crate::domain::types::ConnectionState;

/// Status bar widget that renders connection status, channel info, mode, and counts.
pub struct StatusBar<'a> {
    state: &'a AppState,
}

impl<'a> StatusBar<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    fn connection_indicator(&self) -> Span<'static> {
        let theme = &self.state.theme;
        match &self.state.connection {
            ConnectionState::Connected { .. } => {
                Span::styled("● Connected", ratatui::style::Style::default().fg(theme.status_connected_fg))
            }
            ConnectionState::Connecting => {
                Span::styled("◌ Connecting…", ratatui::style::Style::default().fg(theme.status_connecting_fg))
            }
            ConnectionState::Resuming { .. } => {
                Span::styled("↻ Resuming…", ratatui::style::Style::default().fg(theme.status_connecting_fg))
            }
            ConnectionState::Disconnected => {
                Span::styled("○ Disconnected", ratatui::style::Style::default().fg(theme.status_disconnected_fg))
            }
        }
    }

    fn channel_info(&self) -> Span<'static> {
        let pane = self.state.focused_pane();
        let theme = &self.state.theme;

        let text = match (pane.guild_id, pane.channel_id) {
            (Some(guild_id), Some(channel_id)) => {
                let guild_name = self
                    .state
                    .cache
                    .guilds
                    .get(&guild_id)
                    .map(|g| g.name.as_str())
                    .unwrap_or("Unknown");
                let channel_name = self.state.cache.resolve_channel_name(channel_id);
                format!("{} > #{}", guild_name, channel_name)
            }
            (None, Some(channel_id)) => {
                format!("DM: {}", self.state.cache.resolve_channel_name(channel_id))
            }
            _ => "No channel selected".to_string(),
        };

        Span::styled(text, ratatui::style::Style::default().fg(theme.status_bar_fg))
    }

    fn mode_indicator(&self) -> Span<'static> {
        let theme = &self.state.theme;
        Span::styled(
            format!("[{}]", self.state.input_mode.display_name()),
            ratatui::style::Style::default().fg(theme.status_mode_fg),
        )
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = self.state.theme.status_bar_style();

        // Fill background
        for x in area.left()..area.right() {
            buf[(x, area.y)]
                .set_char(' ')
                .set_style(style);
        }

        let conn = self.connection_indicator();
        let channel = self.channel_info();
        let mode = self.mode_indicator();

        // Build the line: " connection | channel info          [MODE] "
        let separator = Span::styled(" │ ", style);

        let line = Line::from(vec![
            Span::styled(" ", style),
            conn,
            separator.clone(),
            channel,
        ]);

        // Render left-aligned content
        let line_width: u16 = line.width() as u16;
        buf.set_line(area.x, area.y, &line, area.width);

        // Render mode indicator right-aligned
        let mode_width = mode.width() as u16 + 1; // +1 for trailing space
        if area.width > mode_width + line_width {
            let mode_x = area.right() - mode_width;
            let mode_line = Line::from(vec![mode, Span::styled(" ", style)]);
            buf.set_line(mode_x, area.y, &mode_line, mode_width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::config::AppConfig;
    use crate::domain::types::*;
    use crate::input::mode::InputMode;

    fn test_state() -> AppState {
        AppState::new(AppConfig::default())
    }

    fn render_status_bar(state: &AppState) -> Buffer {
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        let widget = StatusBar::new(state);
        widget.render(area, &mut buf);
        buf
    }

    fn buffer_text(buf: &Buffer) -> String {
        let area = buf.area;
        let mut text = String::new();
        for x in area.left()..area.right() {
            let cell = &buf[(x, area.y)];
            text.push_str(cell.symbol());
        }
        text.trim_end().to_string()
    }

    #[test]
    fn renders_disconnected_status() {
        let state = test_state();
        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("Disconnected"), "text was: {}", text);
    }

    #[test]
    fn renders_connected_status() {
        let mut state = test_state();
        state.connection = ConnectionState::Connected {
            session_id: "abc".to_string(),
            resume_url: "wss://example.com".to_string(),
            sequence: 1,
        };
        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("Connected"), "text was: {}", text);
    }

    #[test]
    fn renders_connecting_status() {
        let mut state = test_state();
        state.connection = ConnectionState::Connecting;
        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("Connecting"), "text was: {}", text);
    }

    #[test]
    fn renders_resuming_status() {
        let mut state = test_state();
        state.connection = ConnectionState::Resuming {
            session_id: "abc".to_string(),
            resume_url: "wss://example.com".to_string(),
            sequence: 1,
        };
        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("Resuming"), "text was: {}", text);
    }

    #[test]
    fn renders_no_channel_selected() {
        let state = test_state();
        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("No channel selected"), "text was: {}", text);
    }

    #[test]
    fn renders_channel_info_with_guild() {
        let mut state = test_state();
        let guild_id = Id::new(1);
        let channel_id = Id::new(10);
        state.cache.guilds.insert(
            guild_id,
            CachedGuild {
                id: guild_id,
                name: "Test Server".to_string(),
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

        // Switch to the channel
        state.panes[0].channel_id = Some(channel_id);
        state.panes[0].guild_id = Some(guild_id);

        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("Test Server"), "text was: {}", text);
        assert!(text.contains("#general"), "text was: {}", text);
    }

    #[test]
    fn renders_mode_indicator() {
        let mut state = test_state();
        state.input_mode = InputMode::Normal;
        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("[NORMAL]"), "text was: {}", text);

        state.input_mode = InputMode::Insert;
        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("[INSERT]"), "text was: {}", text);
    }

    #[test]
    fn renders_dm_channel_info() {
        let mut state = test_state();
        let channel_id = Id::new(100);
        state.cache.channels.insert(
            channel_id,
            CachedChannel {
                id: channel_id,
                guild_id: None,
                name: "friend".to_string(),
                kind: twilight_model::channel::ChannelType::Private,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        state.panes[0].channel_id = Some(channel_id);
        state.panes[0].guild_id = None;

        let buf = render_status_bar(&state);
        let text = buffer_text(&buf);
        assert!(text.contains("DM: friend"), "text was: {}", text);
    }
}

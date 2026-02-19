use std::collections::VecDeque;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::Widget,
};

use crate::app::DiscordCache;
use crate::domain::types::*;
use crate::ui::theme::Theme;

/// Message view widget rendering messages from a channel.
pub struct MessageView<'a> {
    messages: &'a VecDeque<CachedMessage>,
    scroll: &'a ScrollState,
    selected_message: Option<usize>,
    theme: &'a Theme,
    cache: &'a DiscordCache,
}

impl<'a> MessageView<'a> {
    pub fn new(
        messages: &'a VecDeque<CachedMessage>,
        scroll: &'a ScrollState,
        selected_message: Option<usize>,
        theme: &'a Theme,
        cache: &'a DiscordCache,
    ) -> Self {
        Self {
            messages,
            scroll,
            selected_message,
            theme,
            cache,
        }
    }

    /// Calculate which messages are visible given the scroll state and area height.
    /// Returns (start_index, end_index) into the messages VecDeque.
    fn visible_range(&self, visible_lines: usize) -> (usize, usize) {
        let msg_count = self.messages.len();
        if msg_count == 0 {
            return (0, 0);
        }

        let offset = match self.scroll {
            ScrollState::Following => 0,
            ScrollState::Manual { offset } => *offset,
        };

        // We display messages from bottom (newest) up.
        // offset=0 means we see the newest messages.
        // Each message takes ~1 line (simplified). For proper wrapping we'd need
        // to pre-calculate line counts, but for now 1 msg = 1 line.

        let end = msg_count.saturating_sub(offset);
        let start = end.saturating_sub(visible_lines);

        (start, end)
    }
}

impl Widget for MessageView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Fill background
        let bg_style = self.theme.base_style();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_char(' ').set_style(bg_style);
            }
        }

        if self.messages.is_empty() {
            let msg = "No messages yet";
            let line = Line::from(Span::styled(msg, self.theme.dim_style()));
            let y = area.y + area.height / 2;
            let x = area.x + (area.width.saturating_sub(msg.len() as u16)) / 2;
            buf.set_line(x, y, &line, area.width);
            return;
        }

        let (start, end) = self.visible_range(area.height as usize);

        let mut y = area.y;
        let mut prev_date: Option<&str> = None;

        for i in start..end {
            if y >= area.bottom() {
                break;
            }

            let msg = &self.messages[i];

            // Date separator
            let msg_date = msg.timestamp.get(..10); // "2024-01-15"
            if msg_date != prev_date && msg_date.is_some() {
                prev_date = msg_date;
                if y < area.bottom() {
                    let date_str = msg_date.unwrap_or("Unknown date");
                    let separator = format!("── {} ──", date_str);
                    let sep_line = Line::from(Span::styled(
                        separator,
                        self.theme.dim_style(),
                    ));
                    let sep_x =
                        area.x + (area.width.saturating_sub(sep_line.width() as u16)) / 2;
                    buf.set_line(sep_x, y, &sep_line, area.width);
                    y += 1;
                    if y >= area.bottom() {
                        break;
                    }
                }
            }

            // Message selection highlight
            // selected_message is an index from the bottom (0 = newest visible)
            let is_selected = self.selected_message.is_some_and(|sel| {
                let msg_index_from_bottom = (end - 1).saturating_sub(i);
                sel == msg_index_from_bottom
            });

            // Author name
            let author_name = self.cache.resolve_user_name(msg.author_id);

            // Timestamp (just time portion)
            let time = msg
                .timestamp
                .get(11..16) // "HH:MM"
                .unwrap_or("??:??");

            // Edited indicator
            let edited = if msg.edited_timestamp.is_some() {
                " (edited)"
            } else {
                ""
            };

            // Build the line
            let mut spans = vec![
                Span::styled(
                    format!("{} ", time),
                    self.theme.message_timestamp_style(),
                ),
                Span::styled(
                    format!("{}: ", author_name),
                    self.theme.message_author_style(),
                ),
                Span::styled(&msg.content, bg_style),
            ];

            if !edited.is_empty() {
                spans.push(Span::styled(
                    edited.to_string(),
                    ratatui::style::Style::default().fg(self.theme.message_edited_fg),
                ));
            }

            let line = Line::from(spans);

            if is_selected {
                // Highlight selected line
                for x in area.left()..area.right() {
                    buf[(x, y)]
                        .set_style(ratatui::style::Style::default().bg(self.theme.message_selected_bg));
                }
            }

            buf.set_line(area.x, y, &line, area.width);

            // Attachment indicators
            for att in &msg.attachments {
                y += 1;
                if y >= area.bottom() {
                    break;
                }
                let att_line = Line::from(Span::styled(
                    format!("  📎 {} ({})", att.filename, format_size(att.size)),
                    self.theme.dim_style(),
                ));
                buf.set_line(area.x, y, &att_line, area.width);
            }

            y += 1;
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Check if the view should auto-scroll (is in Following mode).
pub fn is_following(scroll: &ScrollState) -> bool {
    matches!(scroll, ScrollState::Following)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(id: u64, author_id: u64, content: &str, timestamp: &str) -> CachedMessage {
        CachedMessage {
            id: Id::new(id),
            channel_id: Id::new(1),
            author_id: Id::new(author_id),
            content: content.to_string(),
            timestamp: timestamp.to_string(),
            edited_timestamp: None,
            attachments: vec![],
            embeds: vec![],
            message_reference: None,
            mention_everyone: false,
            mentions: vec![],
        }
    }

    fn make_cache_with_user(id: u64, name: &str) -> DiscordCache {
        let mut cache = DiscordCache::default();
        cache.users.insert(
            Id::new(id),
            CachedUser {
                id: Id::new(id),
                name: name.to_string(),
                discriminator: None,
                display_name: Some(name.to_string()),
                avatar: None,
            },
        );
        cache
    }

    #[test]
    fn empty_messages_shows_placeholder() {
        let messages = VecDeque::new();
        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let widget = MessageView::new(&messages, &scroll, None, &theme, &cache);

        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let mut found = false;
        for y in 0..10 {
            let line: String = (0..40)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect::<String>();
            if line.contains("No messages yet") {
                found = true;
                break;
            }
        }
        assert!(found);
    }

    #[test]
    fn renders_messages_with_author_and_time() {
        let mut messages = VecDeque::new();
        messages.push_back(make_msg(1, 100, "Hello world", "2024-01-15T10:30:00Z"));

        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = make_cache_with_user(100, "Alice");
        let widget = MessageView::new(&messages, &scroll, None, &theme, &cache);

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        // Find the message line
        let mut found_time = false;
        let mut found_author = false;
        let mut found_content = false;
        for y in 0..10 {
            let line: String = (0..60)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect::<String>();
            if line.contains("10:30") {
                found_time = true;
            }
            if line.contains("Alice") {
                found_author = true;
            }
            if line.contains("Hello world") {
                found_content = true;
            }
        }
        assert!(found_time, "Should show time");
        assert!(found_author, "Should show author");
        assert!(found_content, "Should show content");
    }

    #[test]
    fn renders_edited_indicator() {
        let mut msg = make_msg(1, 100, "edited msg", "2024-01-15T10:30:00Z");
        msg.edited_timestamp = Some("2024-01-15T11:00:00Z".to_string());

        let mut messages = VecDeque::new();
        messages.push_back(msg);

        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = make_cache_with_user(100, "Bob");
        let widget = MessageView::new(&messages, &scroll, None, &theme, &cache);

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let mut found = false;
        for y in 0..10 {
            let line: String = (0..60)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect::<String>();
            if line.contains("(edited)") {
                found = true;
                break;
            }
        }
        assert!(found, "Should show edited indicator");
    }

    #[test]
    fn visible_range_following_shows_newest() {
        let mut messages = VecDeque::new();
        for i in 1..=20 {
            messages.push_back(make_msg(
                i,
                100,
                &format!("msg {}", i),
                "2024-01-15T10:00:00Z",
            ));
        }

        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let view = MessageView::new(&messages, &scroll, None, &theme, &cache);

        let (start, end) = view.visible_range(10);
        assert_eq!(end, 20); // ends at the newest
        assert_eq!(start, 10); // shows last 10
    }

    #[test]
    fn visible_range_manual_offset() {
        let mut messages = VecDeque::new();
        for i in 1..=20 {
            messages.push_back(make_msg(
                i,
                100,
                &format!("msg {}", i),
                "2024-01-15T10:00:00Z",
            ));
        }

        let scroll = ScrollState::Manual { offset: 5 };
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let view = MessageView::new(&messages, &scroll, None, &theme, &cache);

        let (start, end) = view.visible_range(10);
        assert_eq!(end, 15); // 20 - 5 = 15
        assert_eq!(start, 5); // 15 - 10 = 5
    }

    #[test]
    fn date_separator_between_different_days() {
        let mut messages = VecDeque::new();
        messages.push_back(make_msg(1, 100, "day1", "2024-01-15T10:00:00Z"));
        messages.push_back(make_msg(2, 100, "day2", "2024-01-16T10:00:00Z"));

        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = make_cache_with_user(100, "User");
        let widget = MessageView::new(&messages, &scroll, None, &theme, &cache);

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let mut sep_count = 0;
        for y in 0..10 {
            let line: String = (0..60)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect::<String>();
            if line.contains("2024-01-1") && line.contains("──") {
                sep_count += 1;
            }
        }
        assert!(sep_count >= 2, "Should have date separators for two days");
    }

    #[test]
    fn is_following_check() {
        assert!(is_following(&ScrollState::Following));
        assert!(!is_following(&ScrollState::Manual { offset: 5 }));
    }

    #[test]
    fn format_size_variants() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn renders_attachment_indicator() {
        let mut msg = make_msg(1, 100, "check this", "2024-01-15T10:00:00Z");
        msg.attachments.push(MessageAttachment {
            filename: "photo.png".to_string(),
            size: 2048,
            url: "https://example.com/photo.png".to_string(),
            content_type: Some("image/png".to_string()),
        });

        let mut messages = VecDeque::new();
        messages.push_back(msg);

        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = make_cache_with_user(100, "User");
        let widget = MessageView::new(&messages, &scroll, None, &theme, &cache);

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let mut found = false;
        for y in 0..10 {
            let line: String = (0..60)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect::<String>();
            if line.contains("photo.png") {
                found = true;
                break;
            }
        }
        assert!(found, "Should show attachment filename");
    }
}

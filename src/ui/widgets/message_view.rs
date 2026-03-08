use std::collections::VecDeque;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::Widget,
};

use crate::domain::cache::DiscordCache;
use crate::domain::types::{CachedMessage, ScrollState};
use crate::ui::theme::Theme;
use crate::ui::widgets::input_box::unicode_width;

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

    /// Compute the number of rendered lines for a message's content spans,
    /// accounting for word wrapping at the given width.
    fn message_content_lines(&self, msg: &CachedMessage, width: usize) -> usize {
        if width == 0 {
            return 1;
        }
        let author_name = self.cache.resolve_user_name(msg.author_id);
        let time = msg.timestamp.get(11..16).unwrap_or("??:??");
        let edited_len = if msg.edited_timestamp.is_some() { 9 } else { 0 }; // " (edited)"

        let total_width = display_width(time) + 1 // "HH:MM "
            + display_width(&author_name) + 2 // "name: "
            + display_width(&msg.content)
            + edited_len;

        total_width.div_ceil(width).max(1)
    }

    /// Calculate which messages are visible given the scroll state and area.
    /// Returns (`start_index`, `end_index`) into the messages `VecDeque`.
    /// Accounts for date separators, attachments, and word wrapping.
    fn visible_range(&self, visible_lines: usize, width: usize) -> (usize, usize) {
        let msg_count = self.messages.len();
        if msg_count == 0 || visible_lines == 0 {
            return (0, 0);
        }

        let offset = match self.scroll {
            ScrollState::Following => 0,
            ScrollState::Manual { offset } => (*offset).min(msg_count.saturating_sub(1)),
        };

        let end = msg_count.saturating_sub(offset);

        // Walk backwards from `end`, counting actual rendered lines per message.
        let mut lines_used = 0;
        let mut start = end;
        while start > 0 {
            let idx = start - 1;
            let msg = &self.messages[idx];

            // Content lines (wrapped) + 1 per attachment
            let mut msg_lines = self.message_content_lines(msg, width) + msg.attachments.len();

            // Date separator: rendered when date differs from previous message,
            // or for the first rendered message (prev_date starts as None).
            if idx == 0 || msg.timestamp.get(..10) != self.messages[idx - 1].timestamp.get(..10) {
                msg_lines += 1;
            }

            if lines_used + msg_lines > visible_lines {
                break;
            }
            lines_used += msg_lines;
            start = idx;
        }

        // The first visible message always gets a date separator in the render
        // loop (prev_date starts as None). If our walk didn't count one because
        // it shares a date with the message before it, we may need to drop the
        // topmost message to fit.
        if start > 0 && start < end {
            let same_date = self.messages[start].timestamp.get(..10)
                == self.messages[start - 1].timestamp.get(..10);
            if same_date && lines_used + 1 > visible_lines && start + 1 < end {
                start += 1;
            }
        }

        (start, end)
    }
}

/// Compute the display width of a string.
fn display_width(s: &str) -> usize {
    s.chars().map(unicode_width).sum()
}

/// Wrap a list of spans into multiple lines, each fitting within `max_width` columns.
fn wrap_spans(spans: Vec<Span<'_>>, max_width: usize) -> Vec<Line<'_>> {
    if max_width == 0 {
        return vec![Line::from(spans)];
    }

    let mut result: Vec<Line> = Vec::new();
    let mut current_line: Vec<Span> = Vec::new();
    let mut current_width: usize = 0;

    for span in spans {
        let style = span.style;
        let text = span.content.into_owned();
        let mut remaining: &str = &text;

        while !remaining.is_empty() {
            let available = max_width.saturating_sub(current_width);
            if available == 0 {
                result.push(Line::from(std::mem::take(&mut current_line)));
                current_width = 0;
                continue;
            }

            // Find how many chars fit in `available` width
            let mut taken_width = 0;
            let mut byte_end = 0;
            for ch in remaining.chars() {
                let cw = unicode_width(ch);
                if taken_width + cw > available {
                    break;
                }
                taken_width += cw;
                byte_end += ch.len_utf8();
            }

            if byte_end == 0 {
                // Wide char doesn't fit — force new line
                if !current_line.is_empty() {
                    result.push(Line::from(std::mem::take(&mut current_line)));
                    current_width = 0;
                }
                // Take at least one char
                let ch = remaining.chars().next().unwrap();
                byte_end = ch.len_utf8();
                taken_width = unicode_width(ch);
            }

            let chunk = &remaining[..byte_end];
            current_line.push(Span::styled(chunk.to_string(), style));
            current_width += taken_width;
            remaining = &remaining[byte_end..];
        }
    }

    if !current_line.is_empty() {
        result.push(Line::from(current_line));
    }

    if result.is_empty() {
        result.push(Line::default());
    }

    result
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

        let width = area.width as usize;
        let (start, end) = self.visible_range(area.height as usize, width);

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
                    let separator = format!("── {date_str} ──");
                    let sep_line = Line::from(Span::styled(separator, self.theme.dim_style()));
                    let sep_x = area.x + (area.width.saturating_sub(sep_line.width() as u16)) / 2;
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

            // Build the spans
            let mut spans: Vec<Span<'_>> = vec![
                Span::styled(format!("{time} "), self.theme.message_timestamp_style()),
                Span::styled(
                    format!("{author_name}: "),
                    self.theme.message_author_style(),
                ),
                Span::styled(msg.content.clone(), bg_style),
            ];

            if !edited.is_empty() {
                spans.push(Span::styled(
                    edited.to_string(),
                    ratatui::style::Style::default().fg(self.theme.message_edited_fg),
                ));
            }

            // Wrap spans across multiple lines
            let wrapped_lines = wrap_spans(spans, width);

            for wrapped_line in &wrapped_lines {
                if y >= area.bottom() {
                    break;
                }

                if is_selected {
                    for x in area.left()..area.right() {
                        buf[(x, y)].set_style(
                            ratatui::style::Style::default().bg(self.theme.message_selected_bg),
                        );
                    }
                }

                buf.set_line(area.x, y, wrapped_line, area.width);
                y += 1;
            }

            // Attachment indicators
            for att in &msg.attachments {
                if y >= area.bottom() {
                    break;
                }
                let att_line = Line::from(Span::styled(
                    format!("  📎 {} ({})", att.filename, format_size(att.size)),
                    self.theme.dim_style(),
                ));
                buf.set_line(area.x, y, &att_line, area.width);
                y += 1;
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
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
    use crate::domain::types::{CachedUser, Id, MessageAttachment};

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
            rendered: None,
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

        let (start, end) = view.visible_range(10, 80);
        assert_eq!(end, 20); // ends at the newest
                             // 9 messages + 1 date separator = 10 lines
        assert_eq!(start, 11);
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

        let (start, end) = view.visible_range(10, 80);
        assert_eq!(end, 15); // 20 - 5 = 15
                             // 9 messages + 1 date separator = 10 lines
        assert_eq!(start, 6);
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
    fn visible_range_with_date_changes() {
        // 4 messages across 2 dates — each date gets a separator
        let mut messages = VecDeque::new();
        messages.push_back(make_msg(1, 100, "a", "2024-01-15T10:00:00Z"));
        messages.push_back(make_msg(2, 100, "b", "2024-01-15T11:00:00Z"));
        messages.push_back(make_msg(3, 100, "c", "2024-01-16T10:00:00Z"));
        messages.push_back(make_msg(4, 100, "d", "2024-01-16T11:00:00Z"));

        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let view = MessageView::new(&messages, &scroll, None, &theme, &cache);

        // Total rendered lines: sep1 + msg1 + msg2 + sep2 + msg3 + msg4 = 6 lines
        let (start, end) = view.visible_range(6, 80);
        assert_eq!(start, 0);
        assert_eq!(end, 4);

        // With only 5 lines available, the top message gets clipped
        let (start, end) = view.visible_range(5, 80);
        assert_eq!(end, 4);
        // sep2 + msg3 + msg4 = 3 lines for date2, +sep1+msg2 = 5 lines
        // Walking: idx=3 (same date as 2) → 1 line, idx=2 (diff date from 1) → 2 lines,
        // idx=1 (same date as 0) → 1 line. Total=4. idx=0 (first) → 2 lines, total=6>5, break.
        // start=1, then check: msg[1] same date as msg[0]? yes. 4+1=5<=5, no bump.
        assert_eq!(start, 1);
    }

    #[test]
    fn visible_range_with_attachments() {
        let mut messages = VecDeque::new();
        let mut msg_with_att = make_msg(1, 100, "file", "2024-01-15T10:00:00Z");
        msg_with_att.attachments.push(MessageAttachment {
            filename: "a.png".to_string(),
            size: 100,
            url: "https://example.com/a.png".to_string(),
            content_type: None,
        });
        msg_with_att.attachments.push(MessageAttachment {
            filename: "b.png".to_string(),
            size: 200,
            url: "https://example.com/b.png".to_string(),
            content_type: None,
        });
        messages.push_back(msg_with_att);
        messages.push_back(make_msg(2, 100, "text", "2024-01-15T11:00:00Z"));

        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let view = MessageView::new(&messages, &scroll, None, &theme, &cache);

        // msg1: date sep(1) + content(1) + 2 attachments(2) = 4 lines
        // msg2: same date → content(1) = 1 line
        // Total = 5 lines
        let (start, end) = view.visible_range(5, 80);
        assert_eq!(start, 0);
        assert_eq!(end, 2);

        // With only 2 lines: can fit msg2(1 line) + date sep for first rendered(1) = 2
        let (start, end) = view.visible_range(2, 80);
        assert_eq!(start, 1);
        assert_eq!(end, 2);
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
    fn wrap_spans_no_wrap_needed() {
        let spans = vec![Span::raw("hello world")];
        let lines = wrap_spans(spans, 80);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn wrap_spans_wraps_long_text() {
        let spans = vec![Span::raw("abcdefghij")]; // 10 chars
        let lines = wrap_spans(spans, 4);
        assert_eq!(lines.len(), 3); // "abcd", "efgh", "ij"
    }

    #[test]
    fn wrap_spans_preserves_styles_across_break() {
        let style1 = ratatui::style::Style::default().fg(ratatui::style::Color::Red);
        let style2 = ratatui::style::Style::default().fg(ratatui::style::Color::Blue);
        let spans = vec![
            Span::styled("aaa", style1),
            Span::styled("bbb", style2),
        ];
        let lines = wrap_spans(spans, 4);
        // "aaa" (3 wide, style1) + "b" (1 wide, style2) = 4 on line 1
        // "bb" (2 wide, style2) on line 2
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn long_message_wraps_in_render() {
        let long_content = "a".repeat(100);
        let mut messages = VecDeque::new();
        messages.push_back(make_msg(1, 100, &long_content, "2024-01-15T10:30:00Z"));

        let scroll = ScrollState::Following;
        let theme = Theme::default();
        let cache = make_cache_with_user(100, "A");
        let widget = MessageView::new(&messages, &scroll, None, &theme, &cache);

        // Width 40 — "10:30 A: " prefix is ~9 chars, content is 100 chars
        // Total ~109 chars → ceil(109/40) = 3 lines + 1 date separator = 4 lines
        let area = Rect::new(0, 0, 40, 20);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        // The content 'a' should appear on multiple lines
        let mut lines_with_a = 0;
        for y in 0..20u16 {
            let line: String = (0..40u16)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            if line.contains('a') {
                lines_with_a += 1;
            }
        }
        assert!(lines_with_a >= 2, "Long message should wrap to multiple lines, got {lines_with_a}");
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

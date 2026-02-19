use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Widget,
};

use crate::app::{AppState, DiscordCache};
use crate::domain::types::InputState;
use crate::input::mode::InputMode;
use crate::ui::theme::Theme;

/// Input box widget for message composition.
/// Shows reply/edit headers and handles cursor display.
pub struct InputBox<'a> {
    input: &'a InputState,
    mode: InputMode,
    theme: &'a Theme,
    cache: &'a DiscordCache,
}

impl<'a> InputBox<'a> {
    pub fn new(state: &'a AppState) -> Self {
        let pane = state.focused_pane();
        Self {
            input: &pane.input,
            mode: state.input_mode,
            theme: &state.theme,
            cache: &state.cache,
        }
    }

    pub fn from_parts(
        input: &'a InputState,
        mode: InputMode,
        theme: &'a Theme,
        cache: &'a DiscordCache,
    ) -> Self {
        Self {
            input,
            mode,
            theme,
            cache,
        }
    }
}

impl Widget for InputBox<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let style = self.theme.input_style();

        // Fill background
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_char(' ').set_style(style);
            }
        }

        let mut current_y = area.y;

        // Render reply/edit header if applicable
        if let Some(msg_id) = self.input.reply_to {
            if current_y < area.bottom() {
                let header = format!("  Replying to message {}", msg_id.get());
                let header_line = Line::from(Span::styled(
                    header,
                    ratatui::style::Style::default()
                        .fg(self.theme.fg_dim)
                        .add_modifier(Modifier::ITALIC),
                ));
                buf.set_line(area.x, current_y, &header_line, area.width);
                current_y += 1;
            }
        } else if let Some(msg_id) = self.input.editing {
            if current_y < area.bottom() {
                let header = format!("  Editing message {}", msg_id.get());
                let header_line = Line::from(Span::styled(
                    header,
                    ratatui::style::Style::default()
                        .fg(self.theme.fg_dim)
                        .add_modifier(Modifier::ITALIC),
                ));
                buf.set_line(area.x, current_y, &header_line, area.width);
                current_y += 1;
            }
        }

        // Render input content or placeholder
        if current_y < area.bottom() {
            if self.input.content.is_empty() {
                let placeholder = if self.mode == InputMode::Insert {
                    "Type a message..."
                } else {
                    "Press 'i' to start typing"
                };
                let line = Line::from(Span::styled(
                    format!(" {}", placeholder),
                    ratatui::style::Style::default().fg(self.theme.input_placeholder_fg),
                ));
                buf.set_line(area.x, current_y, &line, area.width);
            } else {
                let content_display = format!(" {}", &self.input.content);
                let line = Line::from(Span::styled(content_display, style));
                buf.set_line(area.x, current_y, &line, area.width);
            }
        }
    }
}

/// Handle a character insertion into the input state.
pub fn insert_char(input: &mut InputState, c: char) {
    input.content.insert(input.cursor_pos, c);
    input.cursor_pos += c.len_utf8();
    input.cursor_col += unicode_width(c);
}

/// Handle backspace in the input state.
pub fn delete_char_before_cursor(input: &mut InputState) {
    if input.cursor_pos > 0 {
        // Find the previous character boundary
        let before = &input.content[..input.cursor_pos];
        if let Some(c) = before.chars().last() {
            input.cursor_pos -= c.len_utf8();
            input.cursor_col = input.cursor_col.saturating_sub(unicode_width(c));
            input.content.remove(input.cursor_pos);
        }
    }
}

/// Move cursor left by one character.
pub fn move_cursor_left(input: &mut InputState) {
    if input.cursor_pos > 0 {
        let before = &input.content[..input.cursor_pos];
        if let Some(c) = before.chars().last() {
            input.cursor_pos -= c.len_utf8();
            input.cursor_col = input.cursor_col.saturating_sub(unicode_width(c));
        }
    }
}

/// Move cursor right by one character.
pub fn move_cursor_right(input: &mut InputState) {
    if input.cursor_pos < input.content.len() {
        let after = &input.content[input.cursor_pos..];
        if let Some(c) = after.chars().next() {
            input.cursor_pos += c.len_utf8();
            input.cursor_col += unicode_width(c);
        }
    }
}

/// Move cursor to the start of the input.
pub fn move_cursor_home(input: &mut InputState) {
    input.cursor_pos = 0;
    input.cursor_col = 0;
}

/// Move cursor to the end of the input.
pub fn move_cursor_end(input: &mut InputState) {
    input.cursor_pos = input.content.len();
    input.cursor_col = input
        .content
        .chars()
        .map(unicode_width)
        .sum();
}

/// Get the display width of a character.
fn unicode_width(c: char) -> usize {
    // Simple heuristic: CJK characters are 2 wide, everything else is 1
    if ('\u{1100}'..='\u{115F}').contains(&c)
        || ('\u{2E80}'..='\u{A4CF}').contains(&c)
        || ('\u{AC00}'..='\u{D7A3}').contains(&c)
        || ('\u{F900}'..='\u{FAFF}').contains(&c)
        || ('\u{FE10}'..='\u{FE19}').contains(&c)
        || ('\u{FE30}'..='\u{FE6F}').contains(&c)
        || ('\u{FF00}'..='\u{FF60}').contains(&c)
        || ('\u{FFE0}'..='\u{FFE6}').contains(&c)
        || ('\u{20000}'..='\u{2FA1F}').contains(&c)
    {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::Id;

    fn empty_input() -> InputState {
        InputState::default()
    }

    #[test]
    fn insert_char_ascii() {
        let mut input = empty_input();
        insert_char(&mut input, 'h');
        assert_eq!(input.content, "h");
        assert_eq!(input.cursor_pos, 1);
        assert_eq!(input.cursor_col, 1);

        insert_char(&mut input, 'i');
        assert_eq!(input.content, "hi");
        assert_eq!(input.cursor_pos, 2);
        assert_eq!(input.cursor_col, 2);
    }

    #[test]
    fn insert_char_unicode() {
        let mut input = empty_input();
        insert_char(&mut input, '日');
        assert_eq!(input.content, "日");
        assert_eq!(input.cursor_pos, 3); // 3 bytes
        assert_eq!(input.cursor_col, 2); // 2 display columns
    }

    #[test]
    fn delete_char_before_cursor_ascii() {
        let mut input = empty_input();
        insert_char(&mut input, 'a');
        insert_char(&mut input, 'b');
        insert_char(&mut input, 'c');
        assert_eq!(input.content, "abc");

        delete_char_before_cursor(&mut input);
        assert_eq!(input.content, "ab");
        assert_eq!(input.cursor_pos, 2);
    }

    #[test]
    fn delete_char_at_start_is_noop() {
        let mut input = empty_input();
        delete_char_before_cursor(&mut input);
        assert_eq!(input.content, "");
        assert_eq!(input.cursor_pos, 0);
    }

    #[test]
    fn move_cursor_left_and_right() {
        let mut input = empty_input();
        insert_char(&mut input, 'a');
        insert_char(&mut input, 'b');
        insert_char(&mut input, 'c');
        assert_eq!(input.cursor_pos, 3);

        move_cursor_left(&mut input);
        assert_eq!(input.cursor_pos, 2);
        assert_eq!(input.cursor_col, 2);

        move_cursor_left(&mut input);
        assert_eq!(input.cursor_pos, 1);

        move_cursor_right(&mut input);
        assert_eq!(input.cursor_pos, 2);

        // Can't go past end
        move_cursor_right(&mut input);
        move_cursor_right(&mut input);
        move_cursor_right(&mut input);
        assert_eq!(input.cursor_pos, 3);
    }

    #[test]
    fn move_cursor_left_at_start_is_noop() {
        let mut input = empty_input();
        move_cursor_left(&mut input);
        assert_eq!(input.cursor_pos, 0);
        assert_eq!(input.cursor_col, 0);
    }

    #[test]
    fn move_cursor_home_and_end() {
        let mut input = empty_input();
        insert_char(&mut input, 'h');
        insert_char(&mut input, 'e');
        insert_char(&mut input, 'l');
        insert_char(&mut input, 'l');
        insert_char(&mut input, 'o');

        move_cursor_home(&mut input);
        assert_eq!(input.cursor_pos, 0);
        assert_eq!(input.cursor_col, 0);

        move_cursor_end(&mut input);
        assert_eq!(input.cursor_pos, 5);
        assert_eq!(input.cursor_col, 5);
    }

    #[test]
    fn insert_in_middle() {
        let mut input = empty_input();
        insert_char(&mut input, 'a');
        insert_char(&mut input, 'c');

        // Move cursor back to position 1
        move_cursor_left(&mut input);
        assert_eq!(input.cursor_pos, 1);

        insert_char(&mut input, 'b');
        assert_eq!(input.content, "abc");
        assert_eq!(input.cursor_pos, 2);
    }

    #[test]
    fn delete_in_middle() {
        let mut input = empty_input();
        insert_char(&mut input, 'a');
        insert_char(&mut input, 'b');
        insert_char(&mut input, 'c');

        move_cursor_left(&mut input); // cursor at 'c'
        delete_char_before_cursor(&mut input); // delete 'b'
        assert_eq!(input.content, "ac");
        assert_eq!(input.cursor_pos, 1);
    }

    #[test]
    fn render_empty_input_shows_placeholder() {
        let input = empty_input();
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let widget = InputBox::from_parts(&input, InputMode::Normal, &theme, &cache);

        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let text: String = (0..40).map(|x| buf[(x, 0u16)].symbol().to_string()).collect::<String>();
        assert!(
            text.contains("Press 'i' to start typing"),
            "text was: {}",
            text
        );
    }

    #[test]
    fn render_insert_mode_placeholder() {
        let input = empty_input();
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let widget = InputBox::from_parts(&input, InputMode::Insert, &theme, &cache);

        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let text: String = (0..40).map(|x| buf[(x, 0u16)].symbol().to_string()).collect::<String>();
        assert!(
            text.contains("Type a message"),
            "text was: {}",
            text
        );
    }

    #[test]
    fn render_with_content() {
        let mut input = empty_input();
        input.content = "Hello world".to_string();
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let widget = InputBox::from_parts(&input, InputMode::Insert, &theme, &cache);

        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let text: String = (0..40).map(|x| buf[(x, 0u16)].symbol().to_string()).collect::<String>();
        assert!(text.contains("Hello world"), "text was: {}", text);
    }

    #[test]
    fn render_reply_header() {
        let mut input = empty_input();
        input.reply_to = Some(Id::new(42));
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let widget = InputBox::from_parts(&input, InputMode::Insert, &theme, &cache);

        let area = Rect::new(0, 0, 50, 2);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let text: String = (0..50).map(|x| buf[(x, 0u16)].symbol().to_string()).collect::<String>();
        assert!(text.contains("Replying to"), "text was: {}", text);
    }

    #[test]
    fn render_edit_header() {
        let mut input = empty_input();
        input.editing = Some(Id::new(99));
        input.content = "edited text".to_string();
        let theme = Theme::default();
        let cache = DiscordCache::default();
        let widget = InputBox::from_parts(&input, InputMode::Insert, &theme, &cache);

        let area = Rect::new(0, 0, 50, 2);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let line0: String = (0..50).map(|x| buf[(x, 0u16)].symbol().to_string()).collect::<String>();
        assert!(line0.contains("Editing"), "line0 was: {}", line0);

        let line1: String = (0..50).map(|x| buf[(x, 1u16)].symbol().to_string()).collect::<String>();
        assert!(line1.contains("edited text"), "line1 was: {}", line1);
    }
}

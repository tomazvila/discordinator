use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Widget,
};

use crate::app::AppState;
use crate::domain::cache::DiscordCache;
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
/// Covers CJK, emoji, and other wide characters commonly seen in terminals.
pub fn unicode_width(c: char) -> usize {
    // Zero-width characters
    if c == '\u{200B}' // zero-width space
        || c == '\u{200C}' // zero-width non-joiner
        || c == '\u{200D}' // zero-width joiner (ZWJ)
        || c == '\u{FEFF}' // BOM / zero-width no-break space
        || ('\u{FE00}'..='\u{FE0F}').contains(&c) // variation selectors
    {
        return 0;
    }

    // Double-width characters: CJK + emoji + fullwidth forms
    if ('\u{1100}'..='\u{115F}').contains(&c)   // Hangul Jamo
        || ('\u{2E80}'..='\u{A4CF}').contains(&c) // CJK Radicals..Yi Radicals
        || ('\u{AC00}'..='\u{D7A3}').contains(&c) // Hangul Syllables
        || ('\u{F900}'..='\u{FAFF}').contains(&c)  // CJK Compatibility Ideographs
        || ('\u{FE10}'..='\u{FE19}').contains(&c)  // Vertical forms
        || ('\u{FE30}'..='\u{FE6F}').contains(&c)  // CJK Compatibility Forms
        || ('\u{FF00}'..='\u{FF60}').contains(&c)   // Fullwidth Forms
        || ('\u{FFE0}'..='\u{FFE6}').contains(&c)   // Fullwidth Signs
        || ('\u{20000}'..='\u{2FA1F}').contains(&c) // CJK Unified Ext B..Kangxi
        // Emoji ranges (most render as 2 wide in terminals)
        || ('\u{1F300}'..='\u{1F9FF}').contains(&c) // Misc Symbols, Emoticons, etc.
        || ('\u{1FA00}'..='\u{1FA6F}').contains(&c) // Chess Symbols
        || ('\u{1FA70}'..='\u{1FAFF}').contains(&c) // Symbols and Pictographs Ext-A
        || ('\u{2600}'..='\u{27BF}').contains(&c)   // Misc Symbols, Dingbats
        || ('\u{231A}'..='\u{23F3}').contains(&c)   // Misc Technical (clocks etc.)
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

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// Compute cursor_col from content and cursor_pos for verification.
    fn expected_cursor_col(content: &str, cursor_pos: usize) -> usize {
        content[..cursor_pos]
            .chars()
            .map(unicode_width)
            .sum()
    }

    // --- P4.1 & P4.2: cursor_pos is valid UTF-8 boundary and in bounds ---
    proptest! {
        #[test]
        fn cursor_pos_valid_after_inserts(chars in proptest::collection::vec(proptest::char::any(), 0..50)) {
            let mut input = InputState::default();
            for c in &chars {
                insert_char(&mut input, *c);
                prop_assert!(input.cursor_pos <= input.content.len(),
                    "cursor_pos {} > len {}", input.cursor_pos, input.content.len());
                prop_assert!(input.content.is_char_boundary(input.cursor_pos),
                    "cursor_pos {} is not a char boundary", input.cursor_pos);
            }
        }
    }

    // --- P4.3: cursor_col consistency ---
    proptest! {
        #[test]
        fn cursor_col_consistent_after_inserts(chars in proptest::collection::vec(proptest::char::any(), 0..50)) {
            let mut input = InputState::default();
            for c in &chars {
                insert_char(&mut input, *c);
                let expected = expected_cursor_col(&input.content, input.cursor_pos);
                prop_assert_eq!(input.cursor_col, expected,
                    "cursor_col {} != expected {} after inserting {:?}", input.cursor_col, expected, c);
            }
        }
    }

    // --- P4.4: insert then delete is identity ---
    proptest! {
        #[test]
        fn insert_then_delete_is_identity(
            initial in "[a-zA-Z0-9]{0,20}",
            c in proptest::char::any()
        ) {
            let mut input = InputState::default();
            // Set up initial content
            for ch in initial.chars() {
                insert_char(&mut input, ch);
            }
            let saved_content = input.content.clone();
            let saved_pos = input.cursor_pos;
            let saved_col = input.cursor_col;

            insert_char(&mut input, c);
            delete_char_before_cursor(&mut input);

            prop_assert_eq!(&input.content, &saved_content,
                "Content changed: {:?} -> {:?}", saved_content, input.content);
            prop_assert_eq!(input.cursor_pos, saved_pos);
            prop_assert_eq!(input.cursor_col, saved_col);
        }
    }

    // --- P4.5: move_cursor_home sets pos=0, col=0 ---
    proptest! {
        #[test]
        fn move_home_resets_cursor(chars in proptest::collection::vec(proptest::char::any(), 0..30)) {
            let mut input = InputState::default();
            for c in &chars {
                insert_char(&mut input, *c);
            }
            move_cursor_home(&mut input);
            prop_assert_eq!(input.cursor_pos, 0);
            prop_assert_eq!(input.cursor_col, 0);
        }
    }

    // --- P4.6: move_cursor_end sets pos = content.len() ---
    proptest! {
        #[test]
        fn move_end_to_content_len(chars in proptest::collection::vec(proptest::char::any(), 0..30)) {
            let mut input = InputState::default();
            for c in &chars {
                insert_char(&mut input, *c);
            }
            move_cursor_home(&mut input);
            move_cursor_end(&mut input);
            prop_assert_eq!(input.cursor_pos, input.content.len());
            let expected = expected_cursor_col(&input.content, input.cursor_pos);
            prop_assert_eq!(input.cursor_col, expected);
        }
    }

    // --- P4.7: N right from home, N left returns to home ---
    proptest! {
        #[test]
        fn right_then_left_returns_to_home(chars in proptest::collection::vec(proptest::char::any(), 1..20)) {
            let mut input = InputState::default();
            for c in &chars {
                insert_char(&mut input, *c);
            }
            move_cursor_home(&mut input);
            let n = input.content.chars().count();
            for _ in 0..n {
                move_cursor_right(&mut input);
            }
            for _ in 0..n {
                move_cursor_left(&mut input);
            }
            prop_assert_eq!(input.cursor_pos, 0);
            prop_assert_eq!(input.cursor_col, 0);
        }
    }

    // --- P5.1: ASCII printable chars have width 1 ---
    proptest! {
        #[test]
        fn ascii_printable_width_one(c in 0x20u8..0x7F) {
            let ch = c as char;
            prop_assert_eq!(unicode_width(ch), 1, "ASCII {:?} (0x{:02X}) has width != 1", ch, c);
        }
    }

    // --- P5.4: width is always 0, 1, or 2 ---
    proptest! {
        #[test]
        fn width_is_0_1_or_2(c in proptest::char::any()) {
            let w = unicode_width(c);
            prop_assert!(w <= 2, "unicode_width({:?}) = {} > 2", c, w);
        }
    }
}

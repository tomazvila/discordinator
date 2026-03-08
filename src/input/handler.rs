use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::domain::types::{Action, Direction, SplitDirection};
use crate::input::mode::InputMode;

/// Process a key event in the context of the current input mode.
/// Returns an optional Action and the (potentially changed) `InputMode`.
pub fn handle_key_event(key: KeyEvent, mode: InputMode) -> (Option<Action>, InputMode) {
    match mode {
        InputMode::Normal => handle_normal_mode(key),
        InputMode::Insert => handle_insert_mode(key),
        InputMode::Command => handle_command_mode(key),
        InputMode::PanePrefix => handle_pane_prefix_mode(key),
    }
}

fn handle_normal_mode(key: KeyEvent) -> (Option<Action>, InputMode) {
    // Global keybindings first
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('q') => (Some(Action::Quit), InputMode::Normal),
            KeyCode::Char('b') => (None, InputMode::PanePrefix),
            KeyCode::Char('u') => (Some(Action::ScrollUp(10)), InputMode::Normal),
            KeyCode::Char('d') => (Some(Action::ScrollDown(10)), InputMode::Normal),
            _ => (None, InputMode::Normal),
        };
    }

    match key.code {
        // Mode transitions
        KeyCode::Char('i') => (Some(Action::EnterInsertMode), InputMode::Insert),
        KeyCode::Char(':') => (Some(Action::EnterCommandMode), InputMode::Command),

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => (Some(Action::ScrollDown(1)), InputMode::Normal),
        KeyCode::Char('k') | KeyCode::Up => (Some(Action::ScrollUp(1)), InputMode::Normal),
        KeyCode::Char('g') => (Some(Action::ScrollToTop), InputMode::Normal),
        KeyCode::Char('G') => (Some(Action::ScrollToBottom), InputMode::Normal),

        _ => (None, InputMode::Normal),
    }
}

fn handle_insert_mode(key: KeyEvent) -> (Option<Action>, InputMode) {
    match key.code {
        KeyCode::Esc => (Some(Action::EnterNormalMode), InputMode::Normal),
        // Other insert-mode keys (typing, enter to send) are handled
        // by the input box widget directly, not here.
        _ => (None, InputMode::Insert),
    }
}

fn handle_command_mode(key: KeyEvent) -> (Option<Action>, InputMode) {
    match key.code {
        KeyCode::Esc => (Some(Action::EnterNormalMode), InputMode::Normal),
        _ => (None, InputMode::Command),
    }
}

fn handle_pane_prefix_mode(key: KeyEvent) -> (Option<Action>, InputMode) {
    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    let action = match key.code {
        // Pane operations
        KeyCode::Char('"') => Some(Action::SplitPane(SplitDirection::Horizontal)),
        KeyCode::Char('%') => Some(Action::SplitPane(SplitDirection::Vertical)),
        KeyCode::Char('x') => Some(Action::ClosePane),
        KeyCode::Char('o') => Some(Action::FocusNextPane),
        KeyCode::Char('z') => Some(Action::ToggleZoom),
        KeyCode::Char('s') => Some(Action::FocusSidebar),

        // Ctrl+Arrow → resize pane, plain Arrow → directional focus
        KeyCode::Up if has_ctrl => Some(Action::ResizePane(Direction::Up, 1)),
        KeyCode::Down if has_ctrl => Some(Action::ResizePane(Direction::Down, 1)),
        KeyCode::Left if has_ctrl => Some(Action::ResizePane(Direction::Left, 1)),
        KeyCode::Right if has_ctrl => Some(Action::ResizePane(Direction::Right, 1)),

        // Directional focus (plain arrows)
        KeyCode::Up => Some(Action::FocusPaneDirection(Direction::Up)),
        KeyCode::Down => Some(Action::FocusPaneDirection(Direction::Down)),
        KeyCode::Left => Some(Action::FocusPaneDirection(Direction::Left)),
        KeyCode::Right => Some(Action::FocusPaneDirection(Direction::Right)),

        // Esc or any other key cancels
        _ => None,
    };

    // Pane prefix always returns to Normal after one key
    (action, InputMode::Normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    // Normal mode tests
    #[test]
    fn normal_i_enters_insert_mode() {
        let (action, mode) = handle_key_event(key(KeyCode::Char('i')), InputMode::Normal);
        assert_eq!(action, Some(Action::EnterInsertMode));
        assert_eq!(mode, InputMode::Insert);
    }

    #[test]
    fn normal_colon_enters_command_mode() {
        let (action, mode) = handle_key_event(key(KeyCode::Char(':')), InputMode::Normal);
        assert_eq!(action, Some(Action::EnterCommandMode));
        assert_eq!(mode, InputMode::Command);
    }

    #[test]
    fn normal_ctrl_b_enters_pane_prefix() {
        let (action, mode) = handle_key_event(ctrl_key('b'), InputMode::Normal);
        assert_eq!(action, None);
        assert_eq!(mode, InputMode::PanePrefix);
    }

    #[test]
    fn normal_ctrl_q_quits() {
        let (action, mode) = handle_key_event(ctrl_key('q'), InputMode::Normal);
        assert_eq!(action, Some(Action::Quit));
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn normal_j_scrolls_down() {
        let (action, mode) = handle_key_event(key(KeyCode::Char('j')), InputMode::Normal);
        assert_eq!(action, Some(Action::ScrollDown(1)));
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn normal_k_scrolls_up() {
        let (action, mode) = handle_key_event(key(KeyCode::Char('k')), InputMode::Normal);
        assert_eq!(action, Some(Action::ScrollUp(1)));
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn normal_g_scrolls_to_top() {
        let (action, _) = handle_key_event(key(KeyCode::Char('g')), InputMode::Normal);
        assert_eq!(action, Some(Action::ScrollToTop));
    }

    #[test]
    fn normal_shift_g_scrolls_to_bottom() {
        let (action, _) = handle_key_event(key(KeyCode::Char('G')), InputMode::Normal);
        assert_eq!(action, Some(Action::ScrollToBottom));
    }

    #[test]
    fn normal_ctrl_u_half_page_up() {
        let (action, _) = handle_key_event(ctrl_key('u'), InputMode::Normal);
        assert_eq!(action, Some(Action::ScrollUp(10)));
    }

    #[test]
    fn normal_ctrl_d_half_page_down() {
        let (action, _) = handle_key_event(ctrl_key('d'), InputMode::Normal);
        assert_eq!(action, Some(Action::ScrollDown(10)));
    }

    #[test]
    fn normal_arrow_keys_scroll() {
        let (action, _) = handle_key_event(key(KeyCode::Down), InputMode::Normal);
        assert_eq!(action, Some(Action::ScrollDown(1)));

        let (action, _) = handle_key_event(key(KeyCode::Up), InputMode::Normal);
        assert_eq!(action, Some(Action::ScrollUp(1)));
    }

    // Insert mode tests
    #[test]
    fn insert_esc_returns_to_normal() {
        let (action, mode) = handle_key_event(key(KeyCode::Esc), InputMode::Insert);
        assert_eq!(action, Some(Action::EnterNormalMode));
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn insert_typing_stays_in_insert() {
        let (_, mode) = handle_key_event(key(KeyCode::Char('a')), InputMode::Insert);
        assert_eq!(mode, InputMode::Insert);
    }

    // Command mode tests
    #[test]
    fn command_esc_returns_to_normal() {
        let (action, mode) = handle_key_event(key(KeyCode::Esc), InputMode::Command);
        assert_eq!(action, Some(Action::EnterNormalMode));
        assert_eq!(mode, InputMode::Normal);
    }

    // Pane prefix mode tests
    #[test]
    fn pane_prefix_double_quote_splits_horizontal() {
        let (action, mode) = handle_key_event(key(KeyCode::Char('"')), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::SplitPane(SplitDirection::Horizontal)));
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn pane_prefix_percent_splits_vertical() {
        let (action, mode) = handle_key_event(key(KeyCode::Char('%')), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::SplitPane(SplitDirection::Vertical)));
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn pane_prefix_x_closes_pane() {
        let (action, mode) = handle_key_event(key(KeyCode::Char('x')), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::ClosePane));
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn pane_prefix_o_cycles_focus() {
        let (action, _) = handle_key_event(key(KeyCode::Char('o')), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::FocusNextPane));
    }

    #[test]
    fn pane_prefix_z_toggles_zoom() {
        let (action, _) = handle_key_event(key(KeyCode::Char('z')), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::ToggleZoom));
    }

    #[test]
    fn pane_prefix_s_focuses_sidebar() {
        let (action, _) = handle_key_event(key(KeyCode::Char('s')), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::FocusSidebar));
    }

    #[test]
    fn pane_prefix_arrows_focus_direction() {
        let (action, _) = handle_key_event(key(KeyCode::Up), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::FocusPaneDirection(Direction::Up)));

        let (action, _) = handle_key_event(key(KeyCode::Down), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::FocusPaneDirection(Direction::Down)));

        let (action, _) = handle_key_event(key(KeyCode::Left), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::FocusPaneDirection(Direction::Left)));

        let (action, _) = handle_key_event(key(KeyCode::Right), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::FocusPaneDirection(Direction::Right)));
    }

    #[test]
    fn pane_prefix_esc_cancels() {
        let (action, mode) = handle_key_event(key(KeyCode::Esc), InputMode::PanePrefix);
        assert_eq!(action, None);
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn pane_prefix_always_returns_to_normal() {
        // Any key in pane prefix goes back to normal
        let (_, mode) = handle_key_event(key(KeyCode::Char('?')), InputMode::PanePrefix);
        assert_eq!(mode, InputMode::Normal);
    }

    fn ctrl_arrow(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn pane_prefix_ctrl_arrows_resize() {
        let (action, mode) = handle_key_event(ctrl_arrow(KeyCode::Up), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::ResizePane(Direction::Up, 1)));
        assert_eq!(mode, InputMode::Normal);

        let (action, mode) = handle_key_event(ctrl_arrow(KeyCode::Down), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::ResizePane(Direction::Down, 1)));
        assert_eq!(mode, InputMode::Normal);

        let (action, mode) = handle_key_event(ctrl_arrow(KeyCode::Left), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::ResizePane(Direction::Left, 1)));
        assert_eq!(mode, InputMode::Normal);

        let (action, mode) = handle_key_event(ctrl_arrow(KeyCode::Right), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::ResizePane(Direction::Right, 1)));
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn pane_prefix_plain_arrow_still_focuses() {
        // Verify plain arrows still do focus, not resize
        let (action, _) = handle_key_event(key(KeyCode::Up), InputMode::PanePrefix);
        assert_eq!(action, Some(Action::FocusPaneDirection(Direction::Up)));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_keycode() -> impl Strategy<Value = KeyCode> {
        prop_oneof![
            proptest::char::range('a', 'z').prop_map(KeyCode::Char),
            proptest::char::range('A', 'Z').prop_map(KeyCode::Char),
            proptest::char::range('0', '9').prop_map(KeyCode::Char),
            Just(KeyCode::Enter),
            Just(KeyCode::Tab),
            Just(KeyCode::Backspace),
            Just(KeyCode::Delete),
            Just(KeyCode::Up),
            Just(KeyCode::Down),
            Just(KeyCode::Left),
            Just(KeyCode::Right),
            Just(KeyCode::Home),
            Just(KeyCode::End),
            Just(KeyCode::PageUp),
            Just(KeyCode::PageDown),
            Just(KeyCode::Esc),
        ]
    }

    // --- P8.1: PanePrefix always returns to Normal ---
    proptest! {
        #[test]
        fn pane_prefix_always_returns_normal(code in arb_keycode()) {
            let key_event = KeyEvent::new(code, KeyModifiers::NONE);
            let (_, mode) = handle_key_event(key_event, InputMode::PanePrefix);
            prop_assert_eq!(mode, InputMode::Normal, "PanePrefix didn't return to Normal for {:?}", code);
        }
    }

    // --- P8.2: non-Esc keys in Insert stay in Insert ---
    proptest! {
        #[test]
        fn insert_non_esc_stays_insert(c in proptest::char::range('a', 'z')) {
            let key_event = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            let (_, mode) = handle_key_event(key_event, InputMode::Insert);
            prop_assert_eq!(mode, InputMode::Insert, "Insert mode left for char {:?}", c);
        }
    }
}

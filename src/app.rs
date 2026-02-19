use std::collections::{HashMap, VecDeque};
use std::io;

use color_eyre::eyre::Result;
use crossterm::{
    event::{KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::config::AppConfig;
use crate::domain::types::*;
use crate::input::handler::handle_key_event;
use crate::input::mode::InputMode;
use crate::ui::theme::Theme;

/// Sidebar UI state.
#[derive(Debug, Clone, Default)]
pub struct SidebarState {
    /// Index of selected item in the flattened tree.
    pub selected_index: usize,
    /// Scroll offset for the sidebar list.
    pub scroll_offset: usize,
    /// Set of collapsed guild IDs (guild_id → collapsed).
    pub collapsed_guilds: std::collections::HashSet<Id<GuildMarker>>,
}

/// State for a single pane (leaf in the pane tree).
#[derive(Debug, Clone)]
pub struct PaneState {
    pub id: PaneId,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub scroll: ScrollState,
    pub input: InputState,
    /// Index of the selected message (for reply/edit/delete). None = no selection.
    pub selected_message: Option<usize>,
}

impl PaneState {
    pub fn new(id: PaneId) -> Self {
        Self {
            id,
            channel_id: None,
            guild_id: None,
            scroll: ScrollState::Following,
            input: InputState::default(),
            selected_message: None,
        }
    }
}

/// In-memory Discord cache. Minimal version for UI tasks.
/// Task 10 (other worker) will flesh this out fully.
#[derive(Debug, Clone, Default)]
pub struct DiscordCache {
    pub guilds: HashMap<Id<GuildMarker>, CachedGuild>,
    pub channels: HashMap<Id<ChannelMarker>, CachedChannel>,
    pub users: HashMap<Id<UserMarker>, CachedUser>,
    pub guild_order: Vec<Id<GuildMarker>>,
    pub messages: HashMap<Id<ChannelMarker>, VecDeque<CachedMessage>>,
    pub typing: HashMap<Id<ChannelMarker>, Vec<(Id<UserMarker>, std::time::Instant)>>,
    pub channel_guild: HashMap<Id<ChannelMarker>, Id<GuildMarker>>,
    pub read_states: HashMap<Id<ChannelMarker>, ReadState>,
    pub dm_channels: Vec<Id<ChannelMarker>>,
}

impl DiscordCache {
    pub fn resolve_user_name(&self, id: Id<UserMarker>) -> String {
        self.users
            .get(&id)
            .map(|u| {
                u.display_name
                    .as_deref()
                    .unwrap_or(&u.name)
                    .to_string()
            })
            .unwrap_or_else(|| format!("Unknown({})", id.get()))
    }

    pub fn resolve_channel_name(&self, id: Id<ChannelMarker>) -> String {
        self.channels
            .get(&id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("Unknown({})", id.get()))
    }
}

/// Top-level application state. Owned exclusively by the main loop.
#[derive(Debug, Clone)]
pub struct AppState {
    // Discord state
    pub cache: DiscordCache,
    pub connection: ConnectionState,

    // UI state
    pub panes: Vec<PaneState>,
    pub focused_pane: usize,
    pub sidebar: SidebarState,
    pub sidebar_visible: bool,

    // Input
    pub input_mode: InputMode,

    // Settings
    pub config: AppConfig,
    pub theme: Theme,

    // Status
    pub status_message: Option<String>,
    pub status_error: Option<String>,
}

impl AppState {
    /// Create a new AppState with default single pane.
    pub fn new(config: AppConfig) -> Self {
        let theme = Theme::default().with_border_colors(
            &config.pane.active_border_color,
            &config.pane.inactive_border_color,
        );
        let sidebar_visible = config.appearance.show_sidebar;

        Self {
            cache: DiscordCache::default(),
            connection: ConnectionState::Disconnected,
            panes: vec![PaneState::new(PaneId(0))],
            focused_pane: 0,
            sidebar: SidebarState::default(),
            sidebar_visible,
            input_mode: InputMode::Normal,
            config,
            theme,
            status_message: None,
            status_error: None,
        }
    }

    /// Get the currently focused pane.
    pub fn focused_pane(&self) -> &PaneState {
        &self.panes[self.focused_pane]
    }

    /// Get the currently focused pane mutably.
    pub fn focused_pane_mut(&mut self) -> &mut PaneState {
        &mut self.panes[self.focused_pane]
    }
}

/// Top-level App: owns all state + terminal + channels.
/// The main event loop runs inside `App::run()`.
pub struct App {
    pub state: AppState,
    pub dirty: bool,
    pub should_quit: bool,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        Self {
            state: AppState::new(config),
            dirty: true, // Render on first frame
            should_quit: false,
        }
    }

    /// Set up the terminal for TUI mode (raw mode + alternate screen).
    pub fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(terminal)
    }

    /// Restore the terminal to normal mode.
    pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        Ok(())
    }

    /// Handle a terminal key event. Returns true if state was modified.
    pub fn handle_terminal_event(&mut self, key: KeyEvent) -> bool {
        // In insert mode, handle typing keys directly
        if self.state.input_mode == InputMode::Insert {
            match key.code {
                KeyCode::Esc => {
                    return apply_action(Action::EnterNormalMode, &mut self.state);
                }
                KeyCode::Enter => {
                    // Send message
                    let pane = self.state.focused_pane();
                    if let Some(channel_id) = pane.channel_id {
                        let content = pane.input.content.clone();
                        let reply_to = pane.input.reply_to;
                        if !content.is_empty() {
                            return apply_action(
                                Action::SendMessage {
                                    channel_id,
                                    content,
                                    reply_to,
                                },
                                &mut self.state,
                            );
                        }
                    }
                    return false;
                }
                KeyCode::Char(c) => {
                    crate::ui::widgets::input_box::insert_char(
                        &mut self.state.focused_pane_mut().input,
                        c,
                    );
                    return true;
                }
                KeyCode::Backspace => {
                    crate::ui::widgets::input_box::delete_char_before_cursor(
                        &mut self.state.focused_pane_mut().input,
                    );
                    return true;
                }
                KeyCode::Left => {
                    crate::ui::widgets::input_box::move_cursor_left(
                        &mut self.state.focused_pane_mut().input,
                    );
                    return true;
                }
                KeyCode::Right => {
                    crate::ui::widgets::input_box::move_cursor_right(
                        &mut self.state.focused_pane_mut().input,
                    );
                    return true;
                }
                KeyCode::Home => {
                    crate::ui::widgets::input_box::move_cursor_home(
                        &mut self.state.focused_pane_mut().input,
                    );
                    return true;
                }
                KeyCode::End => {
                    crate::ui::widgets::input_box::move_cursor_end(
                        &mut self.state.focused_pane_mut().input,
                    );
                    return true;
                }
                _ => return false,
            }
        }

        // For non-insert modes, use the key handler
        let (action, new_mode) = handle_key_event(key, self.state.input_mode);

        // Update mode if changed
        let mode_changed = new_mode != self.state.input_mode;
        self.state.input_mode = new_mode;

        // Check for quit
        if let Some(Action::Quit) | Some(Action::ForceQuit) = &action {
            self.should_quit = true;
        }

        // Apply the action if any
        if let Some(action) = action {
            let dirty = apply_action(action, &mut self.state);
            dirty || mode_changed
        } else {
            mode_changed
        }
    }
}

/// Apply an action to the app state. Returns true if state was modified (dirty).
pub fn apply_action(action: Action, state: &mut AppState) -> bool {
    match action {
        // Mode transitions
        Action::EnterInsertMode => {
            state.input_mode = InputMode::Insert;
            true
        }
        Action::EnterNormalMode => {
            state.input_mode = InputMode::Normal;
            true
        }
        Action::EnterCommandMode => {
            state.input_mode = InputMode::Command;
            true
        }
        Action::EnterPanePrefix => {
            state.input_mode = InputMode::PanePrefix;
            true
        }

        // Navigation
        Action::SwitchChannel(channel_id) => {
            let guild_id = state.cache.channel_guild.get(&channel_id).copied();
            let pane = state.focused_pane_mut();
            pane.channel_id = Some(channel_id);
            pane.guild_id = guild_id;
            pane.scroll = ScrollState::Following;
            pane.selected_message = None;
            true
        }

        Action::ScrollUp(n) => {
            let pane = state.focused_pane_mut();
            match &mut pane.scroll {
                ScrollState::Following => {
                    pane.scroll = ScrollState::Manual { offset: n };
                }
                ScrollState::Manual { offset } => {
                    *offset = offset.saturating_add(n);
                }
            }
            true
        }

        Action::ScrollDown(n) => {
            let pane = state.focused_pane_mut();
            if let ScrollState::Manual { offset } = &mut pane.scroll {
                let new_offset = offset.saturating_sub(n);
                if new_offset == 0 {
                    pane.scroll = ScrollState::Following;
                } else {
                    *offset = new_offset;
                }
            }
            // Scrolling down when Following is a no-op
            true
        }

        Action::ScrollToTop => {
            let pane = state.focused_pane_mut();
            pane.scroll = ScrollState::Manual { offset: usize::MAX };
            true
        }

        Action::ScrollToBottom => {
            let pane = state.focused_pane_mut();
            pane.scroll = ScrollState::Following;
            true
        }

        // UI toggles
        Action::ToggleSidebar => {
            state.sidebar_visible = !state.sidebar_visible;
            true
        }

        Action::ToggleCommandPalette => {
            // Will be implemented in Phase 3
            true
        }

        // Pane operations (basic for now, full pane tree in Tasks 31-37)
        Action::FocusNextPane => {
            if !state.panes.is_empty() {
                state.focused_pane = (state.focused_pane + 1) % state.panes.len();
            }
            true
        }

        Action::FocusPaneDirection(_dir) => {
            // Full directional focus requires pane tree layout (Task 32)
            // For now, just cycle
            if !state.panes.is_empty() {
                state.focused_pane = (state.focused_pane + 1) % state.panes.len();
            }
            true
        }

        // Message operations produce HTTP requests (handled in app event loop)
        // Here we just track state changes
        Action::SendMessage { .. } => {
            // Clear the input box for the focused pane
            let pane = state.focused_pane_mut();
            pane.input.content.clear();
            pane.input.cursor_pos = 0;
            pane.input.cursor_col = 0;
            pane.input.reply_to = None;
            pane.input.editing = None;
            true
        }

        Action::EditMessage { .. } => true,
        Action::DeleteMessage { .. } => true,

        // Pane operations (stubs — full impl in Tasks 31-37)
        Action::SplitPane(_) => true,
        Action::ClosePane => true,
        Action::ResizePane(_, _) => true,
        Action::ToggleZoom => true,
        Action::SwapPane(_) => true,

        Action::Quit | Action::ForceQuit => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        AppState::new(AppConfig::default())
    }

    #[test]
    fn new_state_has_single_pane() {
        let state = test_state();
        assert_eq!(state.panes.len(), 1);
        assert_eq!(state.focused_pane, 0);
    }

    #[test]
    fn new_state_starts_in_normal_mode() {
        let state = test_state();
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn new_state_starts_disconnected() {
        let state = test_state();
        assert_eq!(state.connection, ConnectionState::Disconnected);
    }

    #[test]
    fn action_enter_insert_mode() {
        let mut state = test_state();
        let dirty = apply_action(Action::EnterInsertMode, &mut state);
        assert!(dirty);
        assert_eq!(state.input_mode, InputMode::Insert);
    }

    #[test]
    fn action_enter_normal_mode() {
        let mut state = test_state();
        state.input_mode = InputMode::Insert;
        let dirty = apply_action(Action::EnterNormalMode, &mut state);
        assert!(dirty);
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn action_enter_command_mode() {
        let mut state = test_state();
        let dirty = apply_action(Action::EnterCommandMode, &mut state);
        assert!(dirty);
        assert_eq!(state.input_mode, InputMode::Command);
    }

    #[test]
    fn action_enter_pane_prefix() {
        let mut state = test_state();
        let dirty = apply_action(Action::EnterPanePrefix, &mut state);
        assert!(dirty);
        assert_eq!(state.input_mode, InputMode::PanePrefix);
    }

    #[test]
    fn action_toggle_sidebar() {
        let mut state = test_state();
        let initial = state.sidebar_visible;
        apply_action(Action::ToggleSidebar, &mut state);
        assert_ne!(state.sidebar_visible, initial);
        apply_action(Action::ToggleSidebar, &mut state);
        assert_eq!(state.sidebar_visible, initial);
    }

    #[test]
    fn action_switch_channel() {
        let mut state = test_state();
        let channel_id = Id::new(12345);
        apply_action(Action::SwitchChannel(channel_id), &mut state);
        assert_eq!(state.focused_pane().channel_id, Some(channel_id));
        assert_eq!(state.focused_pane().scroll, ScrollState::Following);
    }

    #[test]
    fn action_scroll_up_from_following() {
        let mut state = test_state();
        apply_action(Action::ScrollUp(5), &mut state);
        assert_eq!(
            state.focused_pane().scroll,
            ScrollState::Manual { offset: 5 }
        );
    }

    #[test]
    fn action_scroll_up_from_manual() {
        let mut state = test_state();
        apply_action(Action::ScrollUp(5), &mut state);
        apply_action(Action::ScrollUp(3), &mut state);
        assert_eq!(
            state.focused_pane().scroll,
            ScrollState::Manual { offset: 8 }
        );
    }

    #[test]
    fn action_scroll_down_to_following() {
        let mut state = test_state();
        apply_action(Action::ScrollUp(5), &mut state);
        // Scroll down past 0 should return to Following
        apply_action(Action::ScrollDown(10), &mut state);
        assert_eq!(state.focused_pane().scroll, ScrollState::Following);
    }

    #[test]
    fn action_scroll_down_partial() {
        let mut state = test_state();
        apply_action(Action::ScrollUp(10), &mut state);
        apply_action(Action::ScrollDown(3), &mut state);
        assert_eq!(
            state.focused_pane().scroll,
            ScrollState::Manual { offset: 7 }
        );
    }

    #[test]
    fn action_scroll_to_top() {
        let mut state = test_state();
        apply_action(Action::ScrollToTop, &mut state);
        assert_eq!(
            state.focused_pane().scroll,
            ScrollState::Manual {
                offset: usize::MAX
            }
        );
    }

    #[test]
    fn action_scroll_to_bottom() {
        let mut state = test_state();
        apply_action(Action::ScrollUp(100), &mut state);
        apply_action(Action::ScrollToBottom, &mut state);
        assert_eq!(state.focused_pane().scroll, ScrollState::Following);
    }

    #[test]
    fn action_focus_next_pane_wraps() {
        let mut state = test_state();
        state.panes.push(PaneState::new(PaneId(1)));
        state.panes.push(PaneState::new(PaneId(2)));
        assert_eq!(state.focused_pane, 0);

        apply_action(Action::FocusNextPane, &mut state);
        assert_eq!(state.focused_pane, 1);

        apply_action(Action::FocusNextPane, &mut state);
        assert_eq!(state.focused_pane, 2);

        apply_action(Action::FocusNextPane, &mut state);
        assert_eq!(state.focused_pane, 0); // wraps
    }

    #[test]
    fn action_send_message_clears_input() {
        let mut state = test_state();
        {
            let pane = state.focused_pane_mut();
            pane.input.content = "hello world".to_string();
            pane.input.cursor_pos = 11;
            pane.input.reply_to = Some(Id::new(99));
        }

        apply_action(
            Action::SendMessage {
                channel_id: Id::new(1),
                content: "hello world".to_string(),
                reply_to: Some(Id::new(99)),
            },
            &mut state,
        );

        assert!(state.focused_pane().input.content.is_empty());
        assert_eq!(state.focused_pane().input.cursor_pos, 0);
        assert!(state.focused_pane().input.reply_to.is_none());
    }

    #[test]
    fn discord_cache_resolve_user_name() {
        let mut cache = DiscordCache::default();
        cache.users.insert(
            Id::new(1),
            CachedUser {
                id: Id::new(1),
                name: "username".to_string(),
                discriminator: None,
                display_name: Some("Display Name".to_string()),
                avatar: None,
            },
        );
        assert_eq!(cache.resolve_user_name(Id::new(1)), "Display Name");
        assert!(cache.resolve_user_name(Id::new(999)).contains("Unknown"));
    }

    #[test]
    fn discord_cache_resolve_channel_name() {
        let mut cache = DiscordCache::default();
        cache.channels.insert(
            Id::new(10),
            CachedChannel {
                id: Id::new(10),
                guild_id: Some(Id::new(1)),
                name: "general".to_string(),
                kind: twilight_model::channel::ChannelType::GuildText,
                position: 0,
                parent_id: None,
                topic: None,
            },
        );
        assert_eq!(cache.resolve_channel_name(Id::new(10)), "general");
        assert!(cache
            .resolve_channel_name(Id::new(999))
            .contains("Unknown"));
    }

    #[test]
    fn pane_state_new_defaults() {
        let pane = PaneState::new(PaneId(42));
        assert_eq!(pane.id, PaneId(42));
        assert!(pane.channel_id.is_none());
        assert!(pane.guild_id.is_none());
        assert_eq!(pane.scroll, ScrollState::Following);
        assert!(pane.input.content.is_empty());
        assert!(pane.selected_message.is_none());
    }

    #[test]
    fn switch_channel_with_guild_lookup() {
        let mut state = test_state();
        let channel_id = Id::new(10);
        let guild_id = Id::new(1);
        state
            .cache
            .channel_guild
            .insert(channel_id, guild_id);

        apply_action(Action::SwitchChannel(channel_id), &mut state);
        assert_eq!(state.focused_pane().channel_id, Some(channel_id));
        assert_eq!(state.focused_pane().guild_id, Some(guild_id));
    }

    // App struct tests
    use crossterm::event::KeyModifiers;

    fn test_app() -> App {
        App::new(AppConfig::default())
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn app_starts_dirty() {
        let app = test_app();
        assert!(app.dirty);
        assert!(!app.should_quit);
    }

    #[test]
    fn app_ctrl_q_sets_should_quit() {
        let mut app = test_app();
        app.handle_terminal_event(ctrl_key('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn app_dirty_flag_on_key_event() {
        let mut app = test_app();
        app.dirty = false;

        // 'j' scrolls down → should set dirty
        let dirty = app.handle_terminal_event(key(KeyCode::Char('j')));
        assert!(dirty);
    }

    #[test]
    fn app_insert_mode_typing() {
        let mut app = test_app();

        // Enter insert mode
        app.handle_terminal_event(key(KeyCode::Char('i')));
        assert_eq!(app.state.input_mode, InputMode::Insert);

        // Type some text
        app.handle_terminal_event(key(KeyCode::Char('h')));
        app.handle_terminal_event(key(KeyCode::Char('i')));
        assert_eq!(app.state.focused_pane().input.content, "hi");
    }

    #[test]
    fn app_insert_mode_backspace() {
        let mut app = test_app();
        app.handle_terminal_event(key(KeyCode::Char('i'))); // enter insert
        app.handle_terminal_event(key(KeyCode::Char('a')));
        app.handle_terminal_event(key(KeyCode::Char('b')));
        app.handle_terminal_event(key(KeyCode::Backspace));
        assert_eq!(app.state.focused_pane().input.content, "a");
    }

    #[test]
    fn app_insert_mode_esc_returns_to_normal() {
        let mut app = test_app();
        app.handle_terminal_event(key(KeyCode::Char('i'))); // enter insert
        assert_eq!(app.state.input_mode, InputMode::Insert);

        app.handle_terminal_event(key(KeyCode::Esc));
        assert_eq!(app.state.input_mode, InputMode::Normal);
    }

    #[test]
    fn app_insert_mode_enter_sends_message() {
        let mut app = test_app();
        let channel_id = Id::new(100);
        app.state.focused_pane_mut().channel_id = Some(channel_id);

        // Enter insert mode and type
        app.handle_terminal_event(key(KeyCode::Char('i')));
        app.handle_terminal_event(key(KeyCode::Char('h')));
        app.handle_terminal_event(key(KeyCode::Char('i')));
        assert_eq!(app.state.focused_pane().input.content, "hi");

        // Press enter to send
        app.handle_terminal_event(key(KeyCode::Enter));
        // Input should be cleared after send
        assert!(app.state.focused_pane().input.content.is_empty());
    }

    #[test]
    fn app_pane_prefix_mode() {
        let mut app = test_app();

        // Ctrl+b enters pane prefix
        app.handle_terminal_event(ctrl_key('b'));
        assert_eq!(app.state.input_mode, InputMode::PanePrefix);

        // 's' toggles sidebar and returns to normal
        app.handle_terminal_event(key(KeyCode::Char('s')));
        assert_eq!(app.state.input_mode, InputMode::Normal);
    }

    #[test]
    fn app_insert_mode_cursor_movement() {
        let mut app = test_app();
        app.handle_terminal_event(key(KeyCode::Char('i'))); // enter insert
        app.handle_terminal_event(key(KeyCode::Char('a')));
        app.handle_terminal_event(key(KeyCode::Char('b')));
        app.handle_terminal_event(key(KeyCode::Char('c')));

        // Move cursor left
        app.handle_terminal_event(key(KeyCode::Left));
        assert_eq!(app.state.focused_pane().input.cursor_pos, 2);

        // Move cursor right
        app.handle_terminal_event(key(KeyCode::Right));
        assert_eq!(app.state.focused_pane().input.cursor_pos, 3);

        // Home
        app.handle_terminal_event(key(KeyCode::Home));
        assert_eq!(app.state.focused_pane().input.cursor_pos, 0);

        // End
        app.handle_terminal_event(key(KeyCode::End));
        assert_eq!(app.state.focused_pane().input.cursor_pos, 3);
    }
}

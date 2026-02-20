use std::collections::VecDeque;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::Span,
    widgets::{Block, Borders, Widget},
};

use crate::app::AppState;
use crate::domain::pane::{split_area, PaneNode};
use crate::domain::types::PaneId;
use crate::ui::widgets::{input_box::InputBox, message_view::MessageView};

/// Render the pane tree recursively into the given area.
/// Each leaf pane gets its own bordered region with message view + input box.
pub fn render_pane_tree(area: Rect, buf: &mut Buffer, state: &AppState) {
    // If zoomed, render only the zoomed pane at full size
    if let Some(zoom_id) = state.pane_manager.zoom_state {
        render_leaf_pane(area, buf, state, zoom_id);
        return;
    }
    render_node(area, buf, state, &state.pane_manager.root);
}

/// Recursively render a PaneNode into the given area.
fn render_node(area: Rect, buf: &mut Buffer, state: &AppState, node: &PaneNode) {
    match node {
        PaneNode::Leaf(pane) => {
            render_leaf_pane(area, buf, state, pane.id);
        }
        PaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let (first_area, second_area) = split_area(area, *direction, *ratio);
            render_node(first_area, buf, state, first);
            render_node(second_area, buf, state, second);
        }
    }
}

/// Render a single leaf pane with border, title, messages, and input box.
fn render_leaf_pane(area: Rect, buf: &mut Buffer, state: &AppState, pane_id: PaneId) {
    if area.height < 3 || area.width < 5 {
        return;
    }

    let pane = match state.pane_manager.root.find(pane_id) {
        Some(p) => p,
        None => return,
    };

    let is_focused = pane_id == state.pane_manager.focused_pane_id;
    let is_zoomed = state.pane_manager.zoom_state == Some(pane_id);

    // Build title
    let title = build_pane_title(state, pane);

    // Build title with zoom indicator
    let title = if is_zoomed {
        format!("{} [Z]", title)
    } else {
        title
    };

    // Border style based on focus
    let border_style = if is_focused {
        state.theme.active_border_style()
    } else {
        state.theme.inactive_border_style()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(title, border_style.add_modifier(Modifier::BOLD)));

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.height < 2 {
        return;
    }

    // Split inner into message area + input area
    let input_height = if pane.input.reply_to.is_some() || pane.input.editing.is_some() {
        2 // header + input line
    } else {
        1
    };

    let msg_height = inner.height.saturating_sub(input_height);
    let message_area = Rect::new(inner.x, inner.y, inner.width, msg_height);
    let input_area = Rect::new(inner.x, inner.y + msg_height, inner.width, input_height);

    // Render messages
    let empty_deque = VecDeque::new();
    let messages = pane
        .channel_id
        .and_then(|id| state.cache.messages.get(&id))
        .unwrap_or(&empty_deque);

    let msg_view = MessageView::new(
        messages,
        &pane.scroll,
        pane.selected_message,
        &state.theme,
        &state.cache,
    );
    msg_view.render(message_area, buf);

    // Only render input box for focused pane
    if is_focused {
        let input = InputBox::from_parts(
            &pane.input,
            state.input_mode,
            &state.theme,
            &state.cache,
        );
        input.render(input_area, buf);
    }
}

/// Build the pane title string from cache lookups.
fn build_pane_title(
    state: &AppState,
    pane: &crate::domain::pane::Pane,
) -> String {
    if let (Some(guild_id), Some(channel_id)) = (pane.guild_id, pane.channel_id) {
        let guild_name = state
            .cache
            .guilds
            .get(&guild_id)
            .map(|g| g.name.as_str())
            .unwrap_or("Unknown");
        let channel_name = state.cache.resolve_channel_name(channel_id);
        format!(" {} > #{} ", guild_name, channel_name)
    } else if let Some(channel_id) = pane.channel_id {
        format!(" #{} ", state.cache.resolve_channel_name(channel_id))
    } else {
        " Discordinator ".to_string()
    }
}

/// Calculate the positions of all leaf panes (for testing).
pub fn calculate_pane_positions(
    state: &AppState,
    pane_area: Rect,
) -> std::collections::HashMap<PaneId, Rect> {
    state.pane_manager.compute_positions(pane_area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::domain::types::*;
    use std::collections::HashMap;

    #[test]
    fn single_pane_renders_without_panic() {
        let state = AppState::new(AppConfig::default());
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_pane_tree(area, &mut buf, &state);
    }

    #[test]
    fn two_pane_vertical_split_renders() {
        let mut state = AppState::new(AppConfig::default());
        state.pane_manager.split(SplitDirection::Vertical);

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_pane_tree(area, &mut buf, &state);
    }

    #[test]
    fn two_pane_horizontal_split_renders() {
        let mut state = AppState::new(AppConfig::default());
        state.pane_manager.split(SplitDirection::Horizontal);

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_pane_tree(area, &mut buf, &state);
    }

    #[test]
    fn three_pane_nested_renders() {
        let mut state = AppState::new(AppConfig::default());
        let id1 = state.pane_manager.split(SplitDirection::Vertical);
        state.pane_manager.focused_pane_id = id1;
        state.pane_manager.split(SplitDirection::Horizontal);

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_pane_tree(area, &mut buf, &state);
    }

    #[test]
    fn pane_positions_single_pane() {
        let state = AppState::new(AppConfig::default());
        let area = Rect::new(0, 0, 80, 24);
        let positions = calculate_pane_positions(&state, area);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[&PaneId(0)], area);
    }

    #[test]
    fn pane_positions_vertical_split() {
        let mut state = AppState::new(AppConfig::default());
        let id1 = state.pane_manager.split(SplitDirection::Vertical);
        let area = Rect::new(0, 0, 80, 24);
        let positions = calculate_pane_positions(&state, area);
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[&PaneId(0)].width, 40);
        assert_eq!(positions[&id1].width, 40);
    }

    #[test]
    fn focused_pane_has_active_border() {
        let mut state = AppState::new(AppConfig::default());
        state.pane_manager.split(SplitDirection::Vertical);

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_pane_tree(area, &mut buf, &state);

        // Focused pane (PaneId(0)) should have active border style
        // We can verify by checking the border character style at position (0,0)
        let active_style = state.theme.active_border_style();
        let cell = &buf[(0u16, 0u16)];
        assert_eq!(cell.style().fg, active_style.fg);
    }

    #[test]
    fn pane_with_channel_shows_title() {
        let mut state = AppState::new(AppConfig::default());
        let guild_id = Id::new(1);
        let channel_id = Id::new(10);

        state.cache.guilds.insert(
            guild_id,
            CachedGuild {
                id: guild_id,
                name: "TestServer".to_string(),
                icon: None,
                channel_order: vec![channel_id],
                roles: HashMap::new(),
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

        state
            .pane_manager
            .root
            .find_mut(PaneId(0))
            .unwrap()
            .channel_id = Some(channel_id);
        state
            .pane_manager
            .root
            .find_mut(PaneId(0))
            .unwrap()
            .guild_id = Some(guild_id);

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_pane_tree(area, &mut buf, &state);

        // Check that the title contains channel name
        let mut found = false;
        for y in 0..2 {
            let line: String = (0..80u16)
                .map(|x| buf[(x, y as u16)].symbol().to_string())
                .collect();
            if line.contains("general") {
                found = true;
                break;
            }
        }
        assert!(found, "Title should contain channel name 'general'");
    }

    #[test]
    fn zoomed_pane_renders_full_area() {
        let mut state = AppState::new(AppConfig::default());
        state.pane_manager.split(SplitDirection::Vertical);
        state.pane_manager.toggle_zoom();

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_pane_tree(area, &mut buf, &state);

        // Should render without panic — zoomed pane takes full area
    }

    #[test]
    fn small_area_does_not_panic() {
        let state = AppState::new(AppConfig::default());
        let area = Rect::new(0, 0, 3, 2);
        let mut buf = Buffer::empty(area);
        render_pane_tree(area, &mut buf, &state);
    }
}

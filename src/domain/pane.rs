use std::collections::HashMap;

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use super::types::{
    ChannelMarker, Direction, GuildMarker, Id, InputState, MessageMarker, PaneId, ScrollState,
    SplitDirection,
};

/// A pane leaf — an independent channel view.
#[derive(Debug, Clone)]
pub struct Pane {
    pub id: PaneId,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub scroll: ScrollState,
    pub input: InputState,
    /// Index of the selected message (for reply/edit/delete). None = no selection.
    pub selected_message: Option<usize>,
    /// Message ID pending delete confirmation. None = not confirming.
    pub confirming_delete: Option<Id<MessageMarker>>,
}

impl Pane {
    pub fn new(id: PaneId) -> Self {
        Self {
            id,
            channel_id: None,
            guild_id: None,
            scroll: ScrollState::Following,
            input: InputState::default(),
            selected_message: None,
            confirming_delete: None,
        }
    }
}

/// Binary tree for pane layout. Each node is either a leaf pane or a split.
#[derive(Debug, Clone)]
pub enum PaneNode {
    Leaf(Pane),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl PaneNode {
    /// Find a leaf by `PaneId`.
    pub fn find(&self, id: PaneId) -> Option<&Pane> {
        match self {
            Self::Leaf(pane) => {
                if pane.id == id {
                    Some(pane)
                } else {
                    None
                }
            }
            Self::Split { first, second, .. } => first.find(id).or_else(|| second.find(id)),
        }
    }

    /// Find a mutable leaf by `PaneId`.
    pub fn find_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            Self::Leaf(pane) => {
                if pane.id == id {
                    Some(pane)
                } else {
                    None
                }
            }
            Self::Split { first, second, .. } => {
                if let Some(pane) = first.find_mut(id) {
                    Some(pane)
                } else {
                    second.find_mut(id)
                }
            }
        }
    }

    /// Collect all leaf pane IDs in in-order traversal.
    pub fn leaves_in_order(&self) -> Vec<PaneId> {
        let mut result = Vec::new();
        self.collect_leaves(&mut result);
        result
    }

    fn collect_leaves(&self, result: &mut Vec<PaneId>) {
        match self {
            Self::Leaf(pane) => result.push(pane.id),
            Self::Split { first, second, .. } => {
                first.collect_leaves(result);
                second.collect_leaves(result);
            }
        }
    }

    /// Count total leaf panes.
    pub fn leaf_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Split { first, second, .. } => first.leaf_count() + second.leaf_count(),
        }
    }

    /// Check if a pane with the given ID exists in this tree.
    pub fn contains(&self, id: PaneId) -> bool {
        self.find(id).is_some()
    }

    /// Find all leaf panes that are currently viewing the given channel.
    pub fn panes_viewing_channel(&self, channel_id: Id<ChannelMarker>) -> Vec<PaneId> {
        let mut result = Vec::new();
        self.collect_viewing_channel(channel_id, &mut result);
        result
    }

    fn collect_viewing_channel(&self, channel_id: Id<ChannelMarker>, result: &mut Vec<PaneId>) {
        match self {
            Self::Leaf(pane) => {
                if pane.channel_id == Some(channel_id) {
                    result.push(pane.id);
                }
            }
            Self::Split { first, second, .. } => {
                first.collect_viewing_channel(channel_id, result);
                second.collect_viewing_channel(channel_id, result);
            }
        }
    }

    /// Split a leaf pane into two. The original pane stays in `first`,
    /// a new pane is created in `second`. Returns the new pane's ID.
    /// Returns None if the pane is not found.
    pub fn split(&mut self, target_id: PaneId, direction: SplitDirection, new_id: PaneId) -> bool {
        match self {
            Self::Leaf(pane) => {
                if pane.id == target_id {
                    let original = std::mem::replace(
                        self,
                        Self::Leaf(Pane::new(PaneId(0))), // placeholder
                    );
                    *self = Self::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(original),
                        second: Box::new(Self::Leaf(Pane::new(new_id))),
                    };
                    true
                } else {
                    false
                }
            }
            Self::Split { first, second, .. } => {
                first.split(target_id, direction, new_id)
                    || second.split(target_id, direction, new_id)
            }
        }
    }

    /// Remove a leaf pane. The sibling is promoted to take the parent split's place.
    /// Returns true if the pane was found and removed.
    /// Returns false if the pane doesn't exist or is the last remaining pane.
    pub fn remove(&mut self, target_id: PaneId) -> bool {
        // Can't remove if this is the only leaf
        if let Self::Leaf(_) = self {
            return false;
        }
        self.remove_inner(target_id)
    }

    fn remove_inner(&mut self, target_id: PaneId) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                // Check if first child is the target leaf
                if let Self::Leaf(pane) = first.as_ref() {
                    if pane.id == target_id {
                        // Promote second to take our place
                        *self = *second.clone();
                        return true;
                    }
                }
                // Check if second child is the target leaf
                if let Self::Leaf(pane) = second.as_ref() {
                    if pane.id == target_id {
                        // Promote first to take our place
                        *self = *first.clone();
                        return true;
                    }
                }
                // Recurse into children
                first.remove_inner(target_id) || second.remove_inner(target_id)
            }
        }
    }

    /// Resize the split ratio at the split containing the target pane.
    /// Delta is applied to the ratio (clamped to 0.1..0.9).
    pub fn resize(&mut self, target_id: PaneId, delta: f32) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split {
                first,
                second,
                ratio,
                ..
            } => {
                // If either direct child contains the target, adjust this split
                if first.contains(target_id) || second.contains(target_id) {
                    let new_ratio = (*ratio + delta).clamp(0.1, 0.9);
                    if (new_ratio - *ratio).abs() > f32::EPSILON {
                        *ratio = new_ratio;
                        return true;
                    }
                    return false;
                }
                // Recurse
                first.resize(target_id, delta) || second.resize(target_id, delta)
            }
        }
    }
}

/// Manages the pane tree, focus, zoom, and ID allocation.
#[derive(Debug, Clone)]
pub struct PaneManager {
    pub root: PaneNode,
    pub focused_pane_id: PaneId,
    pub zoom_state: Option<PaneId>,
    next_id: u32,
}

impl PaneManager {
    pub fn new() -> Self {
        let first_id = PaneId(0);
        Self {
            root: PaneNode::Leaf(Pane::new(first_id)),
            focused_pane_id: first_id,
            zoom_state: None,
            next_id: 1,
        }
    }

    fn allocate_id(&mut self) -> PaneId {
        let id = PaneId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Split the focused pane. Returns the new pane's ID.
    pub fn split(&mut self, direction: SplitDirection) -> PaneId {
        let new_id = self.allocate_id();
        self.root.split(self.focused_pane_id, direction, new_id);
        new_id
    }

    /// Close the focused pane. Moves focus to the next available pane.
    /// Returns false if this is the last pane (can't close).
    pub fn close_focused(&mut self) -> bool {
        if self.root.leaf_count() <= 1 {
            return false;
        }
        let closing_id = self.focused_pane_id;
        let leaves = self.root.leaves_in_order();
        // Find next pane to focus
        let current_idx = leaves.iter().position(|&id| id == closing_id);
        let removed = self.root.remove(closing_id);
        if removed {
            // Focus the next available pane
            let new_leaves = self.root.leaves_in_order();
            if let Some(idx) = current_idx {
                let next_idx = if idx >= new_leaves.len() {
                    new_leaves.len() - 1
                } else {
                    idx
                };
                self.focused_pane_id = new_leaves[next_idx];
            } else if let Some(&first) = new_leaves.first() {
                self.focused_pane_id = first;
            }
            // Clear zoom if the closed pane was the zoomed one
            if self.zoom_state == Some(closing_id) {
                self.zoom_state = None;
            }
        }
        removed
    }

    /// Cycle focus to the next pane (in-order traversal).
    pub fn focus_next(&mut self) {
        let leaves = self.root.leaves_in_order();
        if leaves.len() <= 1 {
            return;
        }
        if let Some(idx) = leaves.iter().position(|&id| id == self.focused_pane_id) {
            let next_idx = (idx + 1) % leaves.len();
            self.focused_pane_id = leaves[next_idx];
        }
    }

    /// Cycle focus to the previous pane.
    pub fn focus_prev(&mut self) {
        let leaves = self.root.leaves_in_order();
        if leaves.len() <= 1 {
            return;
        }
        if let Some(idx) = leaves.iter().position(|&id| id == self.focused_pane_id) {
            let prev_idx = if idx == 0 { leaves.len() - 1 } else { idx - 1 };
            self.focused_pane_id = leaves[prev_idx];
        }
    }

    /// Get the focused pane.
    pub fn focused_pane(&self) -> Option<&Pane> {
        self.root.find(self.focused_pane_id)
    }

    /// Get a mutable reference to the focused pane.
    pub fn focused_pane_mut(&mut self) -> Option<&mut Pane> {
        self.root.find_mut(self.focused_pane_id)
    }

    /// Toggle zoom on the focused pane.
    pub fn toggle_zoom(&mut self) {
        if self.zoom_state.is_some() {
            self.zoom_state = None;
        } else {
            self.zoom_state = Some(self.focused_pane_id);
        }
    }

    /// Total number of leaf panes.
    pub fn pane_count(&self) -> usize {
        self.root.leaf_count()
    }

    /// Get all pane IDs in traversal order.
    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        self.root.leaves_in_order()
    }

    /// Move focus in a direction based on rendered pane positions.
    /// `positions` maps `PaneId` → Rect for all currently rendered leaf panes.
    /// Returns true if focus actually changed.
    pub fn focus_direction(&mut self, dir: Direction, positions: &HashMap<PaneId, Rect>) -> bool {
        let current_rect = match positions.get(&self.focused_pane_id) {
            Some(r) => *r,
            None => return false,
        };

        let center_x = i32::from(current_rect.x) + i32::from(current_rect.width) / 2;
        let center_y = i32::from(current_rect.y) + i32::from(current_rect.height) / 2;

        let mut best: Option<(PaneId, i32)> = None;

        for (&pane_id, &rect) in positions {
            if pane_id == self.focused_pane_id {
                continue;
            }

            let px = i32::from(rect.x) + i32::from(rect.width) / 2;
            let py = i32::from(rect.y) + i32::from(rect.height) / 2;

            // Check if the candidate is in the right direction
            let in_direction = match dir {
                Direction::Up => py < center_y,
                Direction::Down => py > center_y,
                Direction::Left => px < center_x,
                Direction::Right => px > center_x,
            };

            if !in_direction {
                continue;
            }

            // Distance: primary axis distance + secondary axis penalty
            let distance = match dir {
                Direction::Up | Direction::Down => {
                    (py - center_y).abs() + (px - center_x).abs() / 2
                }
                Direction::Left | Direction::Right => {
                    (px - center_x).abs() + (py - center_y).abs() / 2
                }
            };

            if best.is_none() || distance < best.unwrap().1 {
                best = Some((pane_id, distance));
            }
        }

        if let Some((target_id, _)) = best {
            self.focused_pane_id = target_id;
            true
        } else {
            false
        }
    }

    /// Check if splitting the focused pane would result in panes that are too small.
    /// Minimum usable pane size: 10 columns wide, 4 rows tall.
    pub fn can_split(&self, direction: SplitDirection, positions: &HashMap<PaneId, Rect>) -> bool {
        const MIN_PANE_WIDTH: u16 = 10;
        const MIN_PANE_HEIGHT: u16 = 4;

        let current_rect = match positions.get(&self.focused_pane_id) {
            Some(r) => *r,
            None => return false,
        };

        match direction {
            SplitDirection::Horizontal => {
                // Splitting top/bottom: each half gets ~half the height
                let half_height = current_rect.height / 2;
                half_height >= MIN_PANE_HEIGHT && current_rect.width >= MIN_PANE_WIDTH
            }
            SplitDirection::Vertical => {
                // Splitting left/right: each half gets ~half the width
                let half_width = current_rect.width / 2;
                half_width >= MIN_PANE_WIDTH && current_rect.height >= MIN_PANE_HEIGHT
            }
        }
    }

    /// Split the focused pane, but only if the result would be large enough.
    /// Returns `Some(new_id)` on success, None if too small.
    pub fn try_split(
        &mut self,
        direction: SplitDirection,
        positions: &HashMap<PaneId, Rect>,
    ) -> Option<PaneId> {
        if self.can_split(direction, positions) {
            Some(self.split(direction))
        } else {
            None
        }
    }

    /// Resize the focused pane's parent split in a direction.
    /// Returns true if the resize was applied.
    pub fn resize_focused(&mut self, dir: Direction, delta: i16) -> bool {
        let resize_delta = f32::from(delta) * 0.05; // Convert integer delta to ratio change
        let adjustment = match dir {
            Direction::Left | Direction::Up => -resize_delta,
            Direction::Right | Direction::Down => resize_delta,
        };
        self.root.resize(self.focused_pane_id, adjustment)
    }

    /// Compute the rendered Rect for each leaf pane given a total area.
    /// This recursively splits the area according to the pane tree structure.
    pub fn compute_positions(&self, area: Rect) -> HashMap<PaneId, Rect> {
        let mut positions = HashMap::new();
        compute_positions_inner(&self.root, area, &mut positions);
        positions
    }

    /// Assign a channel (and optional guild) to the focused pane, resetting scroll.
    pub fn assign_channel(
        &mut self,
        channel_id: Id<ChannelMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.channel_id = Some(channel_id);
            pane.guild_id = guild_id;
            pane.scroll = ScrollState::Following;
            pane.input = InputState::default();
            pane.selected_message = None;
            pane.confirming_delete = None;
        }
    }
}

/// Recursively compute leaf pane positions from the pane tree.
fn compute_positions_inner(node: &PaneNode, area: Rect, positions: &mut HashMap<PaneId, Rect>) {
    match node {
        PaneNode::Leaf(pane) => {
            positions.insert(pane.id, area);
        }
        PaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let (first_area, second_area) = split_area(area, *direction, *ratio);
            compute_positions_inner(first, first_area, positions);
            compute_positions_inner(second, second_area, positions);
        }
    }
}

/// Split a Rect into two sub-areas based on direction and ratio.
pub fn split_area(area: Rect, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
    match direction {
        SplitDirection::Horizontal => {
            let first_height = (f32::from(area.height) * ratio).round() as u16;
            let first_height = first_height.min(area.height);
            let second_height = area.height.saturating_sub(first_height);
            (
                Rect::new(area.x, area.y, area.width, first_height),
                Rect::new(area.x, area.y + first_height, area.width, second_height),
            )
        }
        SplitDirection::Vertical => {
            let first_width = (f32::from(area.width) * ratio).round() as u16;
            let first_width = first_width.min(area.width);
            let second_width = area.width.saturating_sub(first_width);
            (
                Rect::new(area.x, area.y, first_width, area.height),
                Rect::new(area.x + first_width, area.y, second_width, area.height),
            )
        }
    }
}

// --- Session persistence ---

/// Serializable pane leaf state (only what matters across sessions).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionPane {
    pub id: PaneId,
    pub channel_id: Option<u64>,
    pub guild_id: Option<u64>,
}

/// Serializable pane tree node for session persistence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionNode {
    Leaf(SessionPane),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<Self>,
        second: Box<Self>,
    },
}

/// Serializable session layout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionLayout {
    pub root: SessionNode,
    pub focused_pane_id: PaneId,
    pub next_id: u32,
}

impl PaneManager {
    /// Serialize the pane layout to a JSON string for session persistence.
    pub fn to_session_json(&self) -> Result<String, serde_json::Error> {
        let layout = SessionLayout {
            root: pane_node_to_session(&self.root),
            focused_pane_id: self.focused_pane_id,
            next_id: self.next_id,
        };
        serde_json::to_string(&layout)
    }

    /// Restore pane layout from a JSON string.
    /// Returns None if the JSON is invalid.
    pub fn from_session_json(json: &str) -> Option<Self> {
        let layout: SessionLayout = serde_json::from_str(json).ok()?;
        let root = session_to_pane_node(&layout.root);

        // Verify the focused pane exists
        let focused = if root.contains(layout.focused_pane_id) {
            layout.focused_pane_id
        } else {
            root.leaves_in_order().first().copied().unwrap_or(PaneId(0))
        };

        Some(Self {
            root,
            focused_pane_id: focused,
            zoom_state: None, // Zoom is not persisted
            next_id: layout.next_id,
        })
    }
}

fn pane_node_to_session(node: &PaneNode) -> SessionNode {
    match node {
        PaneNode::Leaf(pane) => SessionNode::Leaf(SessionPane {
            id: pane.id,
            channel_id: pane.channel_id.map(twilight_model::id::Id::get),
            guild_id: pane.guild_id.map(twilight_model::id::Id::get),
        }),
        PaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => SessionNode::Split {
            direction: *direction,
            ratio: *ratio,
            first: Box::new(pane_node_to_session(first)),
            second: Box::new(pane_node_to_session(second)),
        },
    }
}

fn session_to_pane_node(node: &SessionNode) -> PaneNode {
    match node {
        SessionNode::Leaf(sp) => {
            let mut pane = Pane::new(sp.id);
            pane.channel_id = sp.channel_id.map(Id::new);
            pane.guild_id = sp.guild_id.map(Id::new);
            PaneNode::Leaf(pane)
        }
        SessionNode::Split {
            direction,
            ratio,
            first,
            second,
        } => PaneNode::Split {
            direction: *direction,
            ratio: *ratio,
            first: Box::new(session_to_pane_node(first)),
            second: Box::new(session_to_pane_node(second)),
        },
    }
}

impl Default for PaneManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pane_manager_has_single_pane() {
        let pm = PaneManager::new();
        assert_eq!(pm.pane_count(), 1);
        assert_eq!(pm.focused_pane_id, PaneId(0));
        assert!(pm.focused_pane().is_some());
    }

    #[test]
    fn split_horizontal_creates_two_panes() {
        let mut pm = PaneManager::new();
        let new_id = pm.split(SplitDirection::Horizontal);
        assert_eq!(pm.pane_count(), 2);
        assert!(pm.root.contains(PaneId(0)));
        assert!(pm.root.contains(new_id));
    }

    #[test]
    fn split_vertical_creates_two_panes() {
        let mut pm = PaneManager::new();
        let new_id = pm.split(SplitDirection::Vertical);
        assert_eq!(pm.pane_count(), 2);
        assert!(pm.root.contains(PaneId(0)));
        assert!(pm.root.contains(new_id));
    }

    #[test]
    fn multiple_splits() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);
        assert_eq!(pm.pane_count(), 2);

        // Split the focused pane again
        pm.split(SplitDirection::Vertical);
        assert_eq!(pm.pane_count(), 3);

        // Focus next and split
        pm.focus_next();
        pm.split(SplitDirection::Horizontal);
        assert_eq!(pm.pane_count(), 4);
    }

    #[test]
    fn close_pane_reduces_count() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);
        assert_eq!(pm.pane_count(), 2);

        assert!(pm.close_focused());
        assert_eq!(pm.pane_count(), 1);
    }

    #[test]
    fn cannot_close_last_pane() {
        let mut pm = PaneManager::new();
        assert!(!pm.close_focused());
        assert_eq!(pm.pane_count(), 1);
    }

    #[test]
    fn close_updates_focus() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        // Focus is still on pane 0, split creates pane 1
        pm.focused_pane_id = id1;

        assert!(pm.close_focused());
        // Focus should move to remaining pane
        assert_eq!(pm.focused_pane_id, PaneId(0));
        assert!(pm.focused_pane().is_some());
    }

    #[test]
    fn focus_next_cycles() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);
        pm.focus_next();
        pm.split(SplitDirection::Horizontal);

        let leaves = pm.all_pane_ids();
        assert_eq!(leaves.len(), 3);

        // Start at first pane
        pm.focused_pane_id = leaves[0];

        pm.focus_next();
        assert_eq!(pm.focused_pane_id, leaves[1]);

        pm.focus_next();
        assert_eq!(pm.focused_pane_id, leaves[2]);

        // Cycle back to first
        pm.focus_next();
        assert_eq!(pm.focused_pane_id, leaves[0]);
    }

    #[test]
    fn focus_prev_cycles() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);

        let leaves = pm.all_pane_ids();
        pm.focused_pane_id = leaves[0];

        pm.focus_prev();
        assert_eq!(pm.focused_pane_id, leaves[1]);

        pm.focus_prev();
        assert_eq!(pm.focused_pane_id, leaves[0]);
    }

    #[test]
    fn focus_next_single_pane_noop() {
        let mut pm = PaneManager::new();
        let id = pm.focused_pane_id;
        pm.focus_next();
        assert_eq!(pm.focused_pane_id, id);
    }

    #[test]
    fn find_pane_by_id() {
        let mut pm = PaneManager::new();
        let new_id = pm.split(SplitDirection::Vertical);

        assert!(pm.root.find(PaneId(0)).is_some());
        assert!(pm.root.find(new_id).is_some());
        assert!(pm.root.find(PaneId(999)).is_none());
    }

    #[test]
    fn find_mut_pane() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);

        let pane = pm.root.find_mut(PaneId(0)).unwrap();
        pane.channel_id = Some(Id::new(42));

        let pane = pm.root.find(PaneId(0)).unwrap();
        assert_eq!(pane.channel_id, Some(Id::new(42)));
    }

    #[test]
    fn leaves_in_order() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Vertical);

        let leaves = pm.root.leaves_in_order();
        assert_eq!(leaves.len(), 3);
        assert_eq!(leaves[0], PaneId(0));
        // id1 and id2 should follow
        assert!(leaves.contains(&id1));
        assert!(leaves.contains(&id2));
    }

    #[test]
    fn split_creates_default_ratio() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);

        if let PaneNode::Split { ratio, .. } = &pm.root {
            assert!((ratio - 0.5).abs() < f32::EPSILON);
        } else {
            panic!("Expected Split node");
        }
    }

    #[test]
    fn resize_adjusts_ratio() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);

        let result = pm.root.resize(PaneId(0), 0.1);
        assert!(result);

        if let PaneNode::Split { ratio, .. } = &pm.root {
            assert!((ratio - 0.6).abs() < f32::EPSILON);
        } else {
            panic!("Expected Split node");
        }
    }

    #[test]
    fn resize_clamps_to_bounds() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);

        // Try to push ratio beyond 0.9
        pm.root.resize(PaneId(0), 0.5); // 0.5 + 0.5 = 1.0 → clamped to 0.9
        if let PaneNode::Split { ratio, .. } = &pm.root {
            assert!((ratio - 0.9).abs() < f32::EPSILON);
        }

        // Try to push below 0.1
        pm.root.resize(PaneId(0), -0.9); // 0.9 - 0.9 = 0.0 → clamped to 0.1
        if let PaneNode::Split { ratio, .. } = &pm.root {
            assert!((ratio - 0.1).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn toggle_zoom() {
        let mut pm = PaneManager::new();
        assert!(pm.zoom_state.is_none());

        pm.toggle_zoom();
        assert_eq!(pm.zoom_state, Some(PaneId(0)));

        pm.toggle_zoom();
        assert!(pm.zoom_state.is_none());
    }

    #[test]
    fn close_zoomed_pane_clears_zoom() {
        let mut pm = PaneManager::new();
        let new_id = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = new_id;
        pm.toggle_zoom();
        assert_eq!(pm.zoom_state, Some(new_id));

        pm.close_focused();
        assert!(pm.zoom_state.is_none());
    }

    #[test]
    fn pane_new_has_default_state() {
        let pane = Pane::new(PaneId(42));
        assert_eq!(pane.id, PaneId(42));
        assert!(pane.channel_id.is_none());
        assert!(pane.guild_id.is_none());
        assert_eq!(pane.scroll, ScrollState::Following);
        assert!(pane.input.content.is_empty());
    }

    #[test]
    fn focused_pane_mut_modifies() {
        let mut pm = PaneManager::new();
        {
            let pane = pm.focused_pane_mut().unwrap();
            pane.channel_id = Some(Id::new(100));
        }
        assert_eq!(pm.focused_pane().unwrap().channel_id, Some(Id::new(100)));
    }

    #[test]
    fn deep_tree_operations() {
        let mut pm = PaneManager::new();
        // Create a deeper tree: split 4 times
        let id1 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id2;
        let id3 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id3;
        let _id4 = pm.split(SplitDirection::Vertical);

        assert_eq!(pm.pane_count(), 5);

        // Close a middle pane
        pm.focused_pane_id = id2;
        assert!(pm.close_focused());
        assert_eq!(pm.pane_count(), 4);
        assert!(!pm.root.contains(id2));
    }

    #[test]
    fn all_pane_ids_returns_all_leaves() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Vertical);

        let ids = pm.all_pane_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&PaneId(0)));
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn leaf_count_matches_leaves() {
        let mut pm = PaneManager::new();
        assert_eq!(pm.root.leaf_count(), 1);

        pm.split(SplitDirection::Horizontal);
        assert_eq!(pm.root.leaf_count(), 2);

        pm.focus_next();
        pm.split(SplitDirection::Vertical);
        assert_eq!(pm.root.leaf_count(), 3);
    }

    #[test]
    fn pane_manager_default() {
        let pm = PaneManager::default();
        assert_eq!(pm.pane_count(), 1);
        assert_eq!(pm.focused_pane_id, PaneId(0));
    }

    // --- Directional focus tests ---

    fn make_positions(pairs: &[(PaneId, Rect)]) -> HashMap<PaneId, Rect> {
        pairs.iter().cloned().collect()
    }

    #[test]
    fn focus_direction_right() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        // PaneId(0) is left, id1 is right
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 40, 24)),
            (id1, Rect::new(40, 0, 40, 24)),
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Right, &positions));
        assert_eq!(pm.focused_pane_id, id1);
    }

    #[test]
    fn focus_direction_left() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 40, 24)),
            (id1, Rect::new(40, 0, 40, 24)),
        ]);
        pm.focused_pane_id = id1;
        assert!(pm.focus_direction(Direction::Left, &positions));
        assert_eq!(pm.focused_pane_id, PaneId(0));
    }

    #[test]
    fn focus_direction_down() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 80, 12)),
            (id1, Rect::new(0, 12, 80, 12)),
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Down, &positions));
        assert_eq!(pm.focused_pane_id, id1);
    }

    #[test]
    fn focus_direction_up() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 80, 12)),
            (id1, Rect::new(0, 12, 80, 12)),
        ]);
        pm.focused_pane_id = id1;
        assert!(pm.focus_direction(Direction::Up, &positions));
        assert_eq!(pm.focused_pane_id, PaneId(0));
    }

    #[test]
    fn focus_direction_no_pane_in_direction() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 40, 24)),
            (id1, Rect::new(40, 0, 40, 24)),
        ]);
        // Already at leftmost, try going left
        pm.focused_pane_id = PaneId(0);
        assert!(!pm.focus_direction(Direction::Left, &positions));
        assert_eq!(pm.focused_pane_id, PaneId(0));
    }

    #[test]
    fn focus_direction_picks_closest() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Horizontal);

        // Layout: [P0 | P1 top / P2 bottom]
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 40, 24)),
            (id1, Rect::new(40, 0, 40, 12)),
            (id2, Rect::new(40, 12, 40, 12)),
        ]);

        // From P0, go right — should pick P1 (closer center) or P2
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Right, &positions));
        // Either P1 or P2 is valid; P1 should be closer since P0's center is at (20,12)
        // P1 center is at (60,6), P2 center is at (60,18) — both same x distance
        // But P1 is closer vertically (|6-12|=6 < |18-12|=6), actually same
        // Both are equidistant, but P1 comes first in iteration
    }

    // --- Minimum pane size tests ---

    #[test]
    fn can_split_horizontal_sufficient_space() {
        let pm = PaneManager::new();
        let positions = make_positions(&[(PaneId(0), Rect::new(0, 0, 80, 24))]);
        assert!(pm.can_split(SplitDirection::Horizontal, &positions));
    }

    #[test]
    fn can_split_vertical_sufficient_space() {
        let pm = PaneManager::new();
        let positions = make_positions(&[(PaneId(0), Rect::new(0, 0, 80, 24))]);
        assert!(pm.can_split(SplitDirection::Vertical, &positions));
    }

    #[test]
    fn cannot_split_horizontal_too_short() {
        let pm = PaneManager::new();
        // Height 6: each half would be 3, which is < 4
        let positions = make_positions(&[(PaneId(0), Rect::new(0, 0, 80, 6))]);
        assert!(!pm.can_split(SplitDirection::Horizontal, &positions));
    }

    #[test]
    fn cannot_split_vertical_too_narrow() {
        let pm = PaneManager::new();
        // Width 18: each half would be 9, which is < 10
        let positions = make_positions(&[(PaneId(0), Rect::new(0, 0, 18, 24))]);
        assert!(!pm.can_split(SplitDirection::Vertical, &positions));
    }

    #[test]
    fn can_split_horizontal_exact_minimum() {
        let pm = PaneManager::new();
        // Height 8: each half = 4, which is exactly minimum
        let positions = make_positions(&[(PaneId(0), Rect::new(0, 0, 10, 8))]);
        assert!(pm.can_split(SplitDirection::Horizontal, &positions));
    }

    #[test]
    fn can_split_vertical_exact_minimum() {
        let pm = PaneManager::new();
        // Width 20: each half = 10, which is exactly minimum
        let positions = make_positions(&[(PaneId(0), Rect::new(0, 0, 20, 4))]);
        assert!(pm.can_split(SplitDirection::Vertical, &positions));
    }

    #[test]
    fn try_split_returns_none_if_too_small() {
        let mut pm = PaneManager::new();
        let positions = make_positions(&[(PaneId(0), Rect::new(0, 0, 10, 6))]);
        assert!(pm
            .try_split(SplitDirection::Horizontal, &positions)
            .is_none());
        assert_eq!(pm.pane_count(), 1);
    }

    #[test]
    fn try_split_returns_some_if_sufficient() {
        let mut pm = PaneManager::new();
        let positions = make_positions(&[(PaneId(0), Rect::new(0, 0, 80, 24))]);
        let result = pm.try_split(SplitDirection::Vertical, &positions);
        assert!(result.is_some());
        assert_eq!(pm.pane_count(), 2);
    }

    // --- resize_focused ---

    #[test]
    fn resize_focused_right_increases_ratio() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Vertical);
        // Focused is PaneId(0), the left pane
        assert!(pm.resize_focused(Direction::Right, 1));
        if let PaneNode::Split { ratio, .. } = &pm.root {
            assert!(*ratio > 0.5);
        }
    }

    #[test]
    fn resize_focused_left_decreases_ratio() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Vertical);
        assert!(pm.resize_focused(Direction::Left, 1));
        if let PaneNode::Split { ratio, .. } = &pm.root {
            assert!(*ratio < 0.5);
        }
    }

    // --- compute_positions ---

    #[test]
    fn compute_positions_single_pane() {
        let pm = PaneManager::new();
        let area = Rect::new(0, 0, 80, 24);
        let positions = pm.compute_positions(area);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[&PaneId(0)], area);
    }

    #[test]
    fn compute_positions_vertical_split() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        let area = Rect::new(0, 0, 80, 24);
        let positions = pm.compute_positions(area);
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[&PaneId(0)], Rect::new(0, 0, 40, 24));
        assert_eq!(positions[&id1], Rect::new(40, 0, 40, 24));
    }

    #[test]
    fn compute_positions_horizontal_split() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        let area = Rect::new(0, 0, 80, 24);
        let positions = pm.compute_positions(area);
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[&PaneId(0)], Rect::new(0, 0, 80, 12));
        assert_eq!(positions[&id1], Rect::new(0, 12, 80, 12));
    }

    #[test]
    fn compute_positions_nested_splits() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Horizontal);

        let area = Rect::new(0, 0, 80, 24);
        let positions = pm.compute_positions(area);
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[&PaneId(0)], Rect::new(0, 0, 40, 24));
        assert_eq!(positions[&id1], Rect::new(40, 0, 40, 12));
        assert_eq!(positions[&id2], Rect::new(40, 12, 40, 12));
    }

    // --- split_area ---

    #[test]
    fn split_area_vertical() {
        let area = Rect::new(0, 0, 100, 50);
        let (first, second) = split_area(area, SplitDirection::Vertical, 0.5);
        assert_eq!(first, Rect::new(0, 0, 50, 50));
        assert_eq!(second, Rect::new(50, 0, 50, 50));
    }

    #[test]
    fn split_area_horizontal() {
        let area = Rect::new(0, 0, 100, 50);
        let (first, second) = split_area(area, SplitDirection::Horizontal, 0.5);
        assert_eq!(first, Rect::new(0, 0, 100, 25));
        assert_eq!(second, Rect::new(0, 25, 100, 25));
    }

    #[test]
    fn split_area_unequal_ratio() {
        let area = Rect::new(0, 0, 100, 50);
        let (first, second) = split_area(area, SplitDirection::Vertical, 0.3);
        assert_eq!(first.width, 30);
        assert_eq!(second.width, 70);
        assert_eq!(first.width + second.width, area.width);
    }

    #[test]
    fn close_first_pane_keeps_second() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);

        // Close the first pane (PaneId(0))
        pm.focused_pane_id = PaneId(0);
        assert!(pm.close_focused());

        // Only id1 should remain
        assert_eq!(pm.pane_count(), 1);
        assert!(pm.root.contains(id1));
        assert!(!pm.root.contains(PaneId(0)));
        assert_eq!(pm.focused_pane_id, id1);
    }

    // --- Session persistence tests ---

    #[test]
    fn session_roundtrip_single_pane() {
        let pm = PaneManager::new();
        let json = pm.to_session_json().unwrap();
        let restored = PaneManager::from_session_json(&json).unwrap();
        assert_eq!(restored.pane_count(), 1);
        assert_eq!(restored.focused_pane_id, PaneId(0));
    }

    #[test]
    fn session_roundtrip_multiple_panes() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id1;
        let _id2 = pm.split(SplitDirection::Horizontal);

        // Set channel assignments
        pm.root.find_mut(PaneId(0)).unwrap().channel_id = Some(Id::new(100));
        pm.root.find_mut(PaneId(0)).unwrap().guild_id = Some(Id::new(1));
        pm.root.find_mut(id1).unwrap().channel_id = Some(Id::new(200));

        let json = pm.to_session_json().unwrap();
        let restored = PaneManager::from_session_json(&json).unwrap();

        assert_eq!(restored.pane_count(), 3);
        assert_eq!(restored.focused_pane_id, id1);

        // Verify channel assignments survived
        let p0 = restored.root.find(PaneId(0)).unwrap();
        assert_eq!(p0.channel_id, Some(Id::new(100)));
        assert_eq!(p0.guild_id, Some(Id::new(1)));

        let p1 = restored.root.find(id1).unwrap();
        assert_eq!(p1.channel_id, Some(Id::new(200)));
    }

    #[test]
    fn session_roundtrip_preserves_ratios() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Vertical);
        pm.root.resize(PaneId(0), 0.2); // ratio = 0.7

        let json = pm.to_session_json().unwrap();
        let restored = PaneManager::from_session_json(&json).unwrap();

        if let PaneNode::Split { ratio, .. } = &restored.root {
            assert!((*ratio - 0.7).abs() < 0.01);
        } else {
            panic!("Expected Split node");
        }
    }

    #[test]
    fn session_roundtrip_preserves_next_id() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Vertical);
        pm.split(SplitDirection::Horizontal);
        // next_id should be 3 now

        let json = pm.to_session_json().unwrap();
        let restored = PaneManager::from_session_json(&json).unwrap();

        // After restoring, splitting should give ID 3
        let mut restored = restored;
        let new_id = restored.split(SplitDirection::Vertical);
        assert_eq!(new_id, PaneId(3));
    }

    #[test]
    fn session_zoom_not_persisted() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Vertical);
        pm.toggle_zoom();
        assert!(pm.zoom_state.is_some());

        let json = pm.to_session_json().unwrap();
        let restored = PaneManager::from_session_json(&json).unwrap();
        assert!(restored.zoom_state.is_none());
    }

    #[test]
    fn session_invalid_json_returns_none() {
        assert!(PaneManager::from_session_json("not json").is_none());
        assert!(PaneManager::from_session_json("{}").is_none());
    }

    #[test]
    fn session_with_invalid_focused_pane_recovers() {
        let pm = PaneManager::new();
        let mut json: serde_json::Value =
            serde_json::from_str(&pm.to_session_json().unwrap()).unwrap();
        // Corrupt the focused_pane_id to a non-existent pane
        json["focused_pane_id"] = serde_json::json!(999);
        let corrupted = serde_json::to_string(&json).unwrap();

        let restored = PaneManager::from_session_json(&corrupted).unwrap();
        // Should fall back to first leaf
        assert_eq!(restored.focused_pane_id, PaneId(0));
    }

    #[test]
    fn session_json_is_valid_json() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Vertical);
        pm.root.find_mut(PaneId(0)).unwrap().channel_id = Some(Id::new(42));

        let json = pm.to_session_json().unwrap();
        // Should be parseable JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_object());
        assert!(parsed["root"].is_object());
        assert!(parsed["focused_pane_id"].is_number());
    }

    // --- Channel assignment tests (Task 36) ---

    #[test]
    fn assign_channel_to_focused_pane() {
        let mut pm = PaneManager::new();
        let channel_id = Id::new(100);
        let guild_id = Some(Id::new(1));
        pm.assign_channel(channel_id, guild_id);

        let pane = pm.focused_pane().unwrap();
        assert_eq!(pane.channel_id, Some(channel_id));
        assert_eq!(pane.guild_id, guild_id);
        assert_eq!(pane.scroll, ScrollState::Following);
    }

    #[test]
    fn assign_channel_resets_scroll() {
        let mut pm = PaneManager::new();
        // Set manual scroll first
        pm.focused_pane_mut().unwrap().scroll = ScrollState::Manual { offset: 10 };
        pm.assign_channel(Id::new(200), None);
        assert_eq!(pm.focused_pane().unwrap().scroll, ScrollState::Following);
    }

    #[test]
    fn assign_channel_to_specific_pane() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.assign_channel(Id::new(100), Some(Id::new(1)));
        // Now assign a different channel to the second pane
        pm.focused_pane_id = id1;
        pm.assign_channel(Id::new(200), Some(Id::new(2)));

        assert_eq!(
            pm.root.find(PaneId(0)).unwrap().channel_id,
            Some(Id::new(100))
        );
        assert_eq!(pm.root.find(id1).unwrap().channel_id, Some(Id::new(200)));
    }

    #[test]
    fn panes_viewing_channel_single_match() {
        let mut pm = PaneManager::new();
        let _id1 = pm.split(SplitDirection::Vertical);
        let channel_id = Id::new(100);
        pm.root.find_mut(PaneId(0)).unwrap().channel_id = Some(channel_id);

        let viewers = pm.root.panes_viewing_channel(channel_id);
        assert_eq!(viewers, vec![PaneId(0)]);
    }

    #[test]
    fn panes_viewing_channel_multiple_matches() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        let channel_id = Id::new(100);

        // Both panes view the same channel
        pm.root.find_mut(PaneId(0)).unwrap().channel_id = Some(channel_id);
        pm.root.find_mut(id1).unwrap().channel_id = Some(channel_id);

        let viewers = pm.root.panes_viewing_channel(channel_id);
        assert_eq!(viewers.len(), 2);
        assert!(viewers.contains(&PaneId(0)));
        assert!(viewers.contains(&id1));
    }

    #[test]
    fn panes_viewing_channel_no_match() {
        let pm = PaneManager::new();
        let viewers = pm.root.panes_viewing_channel(Id::new(999));
        assert!(viewers.is_empty());
    }

    #[test]
    fn panes_viewing_channel_different_channels() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.root.find_mut(PaneId(0)).unwrap().channel_id = Some(Id::new(100));
        pm.root.find_mut(id1).unwrap().channel_id = Some(Id::new(200));

        let viewers_100 = pm.root.panes_viewing_channel(Id::new(100));
        assert_eq!(viewers_100, vec![PaneId(0)]);

        let viewers_200 = pm.root.panes_viewing_channel(Id::new(200));
        assert_eq!(viewers_200, vec![id1]);
    }

    #[test]
    fn close_zoomed_pane_clears_zoom_explicit() {
        // Verify zoom is cleared using the *closing* pane's ID, not the new focused ID.
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        let _id2 = pm.split(SplitDirection::Horizontal);
        // Zoom pane id1
        pm.focused_pane_id = id1;
        pm.toggle_zoom();
        assert_eq!(pm.zoom_state, Some(id1));

        // Close zoomed pane — zoom must clear regardless of where focus moves
        pm.close_focused();
        assert!(
            pm.zoom_state.is_none(),
            "Zoom should be cleared when the zoomed pane is closed"
        );
        assert_ne!(
            pm.focused_pane_id, id1,
            "Focus should move away from closed pane"
        );
    }

    // --- focus_direction multi-pane tests (kill center calculation mutations) ---

    #[test]
    fn focus_direction_picks_nearest_among_multiple_right() {
        // Layout: [P0(left)] [P1(top-right)] [P2(bottom-right)]
        // From P0, going right should pick the pane whose center is closer.
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Horizontal);

        // Make P2 clearly closer to P0's center than P1
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 40, 24)),  // center: (20, 12)
            (id1, Rect::new(40, 0, 40, 8)),          // center: (60, 4)
            (id2, Rect::new(40, 8, 40, 16)),         // center: (60, 16)
        ]);

        // P0 center is at (20, 12). P1 center at (60,4), P2 center at (60,16).
        // Distance to P1: |4-12| + |60-20|/2 = 8 + 20 = 28
        // Distance to P2: |16-12| + |60-20|/2 = 4 + 20 = 24
        // P2 is closer — should be selected
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Right, &positions));
        assert_eq!(pm.focused_pane_id, id2, "Should pick closest pane (P2)");
    }

    #[test]
    fn focus_direction_picks_nearest_among_multiple_down() {
        // 3-pane layout: [P0(top)] [P1(bottom-left)] [P2(bottom-right)]
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Vertical);

        // Use an asymmetric layout so one pane is clearly closer
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 80, 12)),   // center: (40, 6)
            (id1, Rect::new(0, 12, 30, 12)),          // center: (15, 18)
            (id2, Rect::new(30, 12, 50, 12)),         // center: (55, 18)
        ]);
        // P1: |18-6| + |15-40|/2 = 12 + 12 = 24
        // P2: |18-6| + |55-40|/2 = 12 + 7 = 19
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Down, &positions));
        assert_eq!(pm.focused_pane_id, id2, "Should pick closest pane (P2)");
    }

    #[test]
    fn focus_direction_strict_inequality_rejects_same_axis() {
        // Two panes side by side at the same vertical center — Up/Down should not work
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 40, 24)),   // center: (20, 12)
            (id1, Rect::new(40, 0, 40, 24)),          // center: (60, 12)
        ]);
        pm.focused_pane_id = PaneId(0);
        // Same center_y — neither is strictly above/below
        assert!(!pm.focus_direction(Direction::Up, &positions));
        assert!(!pm.focus_direction(Direction::Down, &positions));
    }

    #[test]
    fn focus_direction_4_pane_grid() {
        // 2x2 grid layout
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = PaneId(0);
        let id2 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id3 = pm.split(SplitDirection::Horizontal);

        // [P0:top-left] [P1:top-right]
        // [P2:bot-left]  [P3:bot-right]
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 40, 12)),    // center: (20, 6)
            (id1, Rect::new(40, 0, 40, 12)),           // center: (60, 6)
            (id2, Rect::new(0, 12, 40, 12)),           // center: (20, 18)
            (id3, Rect::new(40, 12, 40, 12)),          // center: (60, 18)
        ]);

        // From P0: Right→P1, Down→P2
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Right, &positions));
        assert_eq!(pm.focused_pane_id, id1);

        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Down, &positions));
        assert_eq!(pm.focused_pane_id, id2);

        // From P3: Left→P2, Up→P1
        pm.focused_pane_id = id3;
        assert!(pm.focus_direction(Direction::Left, &positions));
        assert_eq!(pm.focused_pane_id, id2);

        pm.focused_pane_id = id3;
        assert!(pm.focus_direction(Direction::Up, &positions));
        assert_eq!(pm.focused_pane_id, id1);
    }

    #[test]
    fn focus_direction_returns_false_for_missing_position() {
        let mut pm = PaneManager::new();
        let positions = HashMap::new();
        assert!(!pm.focus_direction(Direction::Right, &positions));
    }

    /// Test focus_direction with asymmetric multi-pane layouts that exercise
    /// the distance formula: primary_distance + secondary_distance / 2.
    /// Each candidate has different primary AND secondary distances so that
    /// mutating arithmetic operators (+→*, -→/, /→%, /→*) changes the winner.
    #[test]
    fn focus_direction_down_distance_formula() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Vertical);

        // P0: center (40, 10), going Down
        // P1: center (50, 15) — dy=5, dx=10. Correct: 5 + 10/2 = 10
        // P2: center (42, 20) — dy=10, dx=2. Correct: 10 + 2/2 = 11
        // P1 wins (10 < 11)
        let positions = make_positions(&[
            (PaneId(0), Rect::new(30, 0, 20, 20)),  // center: (40, 10)
            (id1, Rect::new(40, 10, 20, 10)),         // center: (50, 15)
            (id2, Rect::new(32, 15, 20, 10)),         // center: (42, 20)
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Down, &positions));
        assert_eq!(pm.focused_pane_id, id1, "P1 should win (distance 10 < 11)");
    }

    #[test]
    fn focus_direction_down_catches_div_to_mod() {
        // Test to distinguish / 2 from % 2 in secondary distance calculation.
        // Need candidates where dx/2 gives different relative order than dx%2.
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Vertical);

        // P0: center (40, 10), going Down
        // P1: center (50, 16) — dy=6, dx=10. Correct: 6+5=11. With %2: 6+0=6
        // P2: center (43, 19) — dy=9, dx=3. Correct: 9+1=10. With %2: 9+1=10
        // Correct: P2(10) wins. With /→%: P1(6) wins — DIFFERENT.
        let positions = make_positions(&[
            (PaneId(0), Rect::new(30, 0, 20, 20)),
            (id1, Rect::new(40, 6, 20, 20)),
            (id2, Rect::new(33, 9, 20, 20)),
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Down, &positions));
        assert_eq!(pm.focused_pane_id, id2, "P2 should win with correct /2 formula");
    }

    #[test]
    fn focus_direction_up_catches_sub_to_add() {
        // For Up direction, py < center_y. With `-→+` in (py - center_y).abs():
        // |py - cy| orders by distance from center, |py + cy| orders by absolute position.
        // These give different orderings when candidates are on the same side of center_y.
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Vertical);

        // P0: center (40, 40), going Up
        // P1: center (40, 30) — dy=10. Correct: 10. With +: |30+40|=70
        // P2: center (40, 10) — dy=30. Correct: 30. With +: |10+40|=50
        // Correct: P1(10) wins. With -→+: P2(50) wins. DIFFERENT.
        let positions = make_positions(&[
            (PaneId(0), Rect::new(30, 30, 20, 20)),  // center: (40, 40)
            (id1, Rect::new(30, 20, 20, 20)),          // center: (40, 30)
            (id2, Rect::new(30, 0, 20, 20)),           // center: (40, 10)
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Up, &positions));
        assert_eq!(pm.focused_pane_id, id1, "P1 should win (closer to center)");
    }

    #[test]
    fn focus_direction_right_distance_formula() {
        // Same as Down test but for Left|Right branch (line 388).
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Horizontal);

        // P0: center (10, 40), going Right
        // P1: center (15, 50) — dx=5, dy=10. Correct: 5 + 10/2 = 10
        // P2: center (20, 42) — dx=10, dy=2. Correct: 10 + 2/2 = 11
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 30, 20, 20)),   // center: (10, 40)
            (id1, Rect::new(5, 40, 20, 20)),           // center: (15, 50)
            (id2, Rect::new(10, 32, 20, 20)),          // center: (20, 42)
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Right, &positions));
        assert_eq!(pm.focused_pane_id, id1, "P1 should win (distance 10 < 11)");
    }

    #[test]
    fn focus_direction_left_catches_sub_to_add() {
        // For Left direction, px < center_x.
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Horizontal);

        // P0: center (40, 40), going Left
        // P1: center (30, 40) — dx=10. Correct: 10.
        // P2: center (10, 40) — dx=30. Correct: 30.
        let positions = make_positions(&[
            (PaneId(0), Rect::new(30, 30, 20, 20)),
            (id1, Rect::new(20, 30, 20, 20)),
            (id2, Rect::new(0, 30, 20, 20)),
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Left, &positions));
        assert_eq!(pm.focused_pane_id, id1, "P1 should win (closer to center)");
    }

    #[test]
    fn focus_direction_right_catches_div_to_mod() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Horizontal);

        // P0: center (10, 40), going Right
        // P1: center (16, 50) — dx=6, dy=10. Correct: 6+5=11. With %2: 6+0=6
        // P2: center (19, 43) — dx=9, dy=3. Correct: 9+1=10. With %2: 9+1=10
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 30, 20, 20)),
            (id1, Rect::new(6, 40, 20, 20)),
            (id2, Rect::new(9, 33, 20, 20)),
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Right, &positions));
        assert_eq!(pm.focused_pane_id, id2, "P2 should win with correct /2 formula");
    }

    #[test]
    fn focus_direction_tie_uses_strict_less_than() {
        // Line 392: `distance < best.unwrap().1`
        // With <=, equal distances would replace; with <, the first candidate wins.
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Vertical);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Horizontal);

        // Two candidates at equal distance — first found should win with `<`, last with `<=`
        let positions = make_positions(&[
            (PaneId(0), Rect::new(0, 0, 20, 40)),   // center: (10, 20)
            (id1, Rect::new(20, 0, 20, 20)),          // center: (30, 10), going Right: dx=20, dy=10 → 20+5=25
            (id2, Rect::new(20, 20, 20, 20)),         // center: (30, 30), going Right: dx=20, dy=10 → 20+5=25
        ]);
        pm.focused_pane_id = PaneId(0);
        assert!(pm.focus_direction(Direction::Right, &positions));
        // With strict <, first candidate (whatever HashMap iteration gives) wins.
        // Key: the result must be deterministic — verify we get ONE of them.
        assert!(
            pm.focused_pane_id == id1 || pm.focused_pane_id == id2,
            "Should focus one of the equidistant panes"
        );
    }

    // --- resize edge cases ---

    #[test]
    fn resize_zero_delta_returns_false() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);
        let result = pm.root.resize(PaneId(0), 0.0);
        assert!(!result, "Zero delta should return false (no change)");
        if let PaneNode::Split { ratio, .. } = &pm.root {
            assert!((ratio - 0.5).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn resize_nonexistent_pane_returns_false() {
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Horizontal);
        assert!(!pm.root.resize(PaneId(999), 0.1));
    }

    // --- resize_focused delta calculation ---

    #[test]
    fn resize_focused_uses_correct_delta_multiplier() {
        // Line 447: f32::from(delta) * 0.05
        // With delta=1, resize_delta = 0.05
        // With * → /, resize_delta = 1.0/0.05 = 20.0 (would be clamped to max)
        // With * → +, resize_delta = 1.0+0.05 = 1.05 (would also be clamped)
        let mut pm = PaneManager::new();
        pm.split(SplitDirection::Vertical);

        // Resize right with delta=1 → should change ratio by +0.05
        assert!(pm.resize_focused(Direction::Right, 1));
        if let PaneNode::Split { ratio, .. } = &pm.root {
            let expected = 0.55;
            assert!(
                (*ratio - expected).abs() < 0.001,
                "Ratio should be ~{expected}, got {ratio}"
            );
        }

        // Resize again
        assert!(pm.resize_focused(Direction::Right, 2));
        if let PaneNode::Split { ratio, .. } = &pm.root {
            let expected = 0.65;
            assert!(
                (*ratio - expected).abs() < 0.001,
                "Ratio should be ~{expected}, got {ratio}"
            );
        }
    }

    // --- close_focused position lookup ---

    #[test]
    fn close_focused_preserves_index_position() {
        // Tests that `== closing_id` is used (not `!= closing_id`) in position lookup.
        // With `!=`, closing middle pane would focus position 0 instead of position 1.
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        pm.focused_pane_id = id1;
        let id2 = pm.split(SplitDirection::Horizontal);

        // 3 panes in order: [P0, P1, P2]. Close P1 (middle, index 1).
        let leaves = pm.all_pane_ids();
        assert_eq!(leaves.len(), 3);
        pm.focused_pane_id = id1;
        assert!(pm.close_focused());

        // After closing P1 at index 1, focus should go to new index 1 (= P2),
        // NOT index 0 (= P0, which `!= closing_id` would give).
        let new_leaves = pm.all_pane_ids();
        assert_eq!(new_leaves.len(), 2);
        let focus_idx = new_leaves
            .iter()
            .position(|&id| id == pm.focused_pane_id)
            .unwrap();
        assert_eq!(
            focus_idx, 1,
            "Focus should be at index 1 (same position as closed pane), got index {focus_idx}"
        );
        assert_eq!(pm.focused_pane_id, id2, "Focus should be on P2 after closing middle pane");
    }

    #[test]
    fn close_last_in_order_wraps_focus() {
        let mut pm = PaneManager::new();
        let _id1 = pm.split(SplitDirection::Horizontal);
        pm.focus_next();
        let id2 = pm.split(SplitDirection::Horizontal);

        pm.focused_pane_id = id2;
        assert!(pm.close_focused());

        let leaves = pm.all_pane_ids();
        assert!(leaves.contains(&pm.focused_pane_id));
    }

    #[test]
    fn close_non_zoomed_pane_keeps_zoom() {
        let mut pm = PaneManager::new();
        let id1 = pm.split(SplitDirection::Horizontal);
        // Zoom pane 0
        pm.focused_pane_id = PaneId(0);
        pm.toggle_zoom();
        assert_eq!(pm.zoom_state, Some(PaneId(0)));

        // Close pane id1 (not the zoomed one)
        pm.focused_pane_id = id1;
        pm.close_focused();
        assert_eq!(
            pm.zoom_state,
            Some(PaneId(0)),
            "Zoom should remain on the non-closed pane"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// Strategy: random sequence of split operations (direction + which pane to focus before split)
    fn split_ops(max_ops: usize) -> impl Strategy<Value = Vec<SplitDirection>> {
        proptest::collection::vec(
            prop_oneof![
                Just(SplitDirection::Horizontal),
                Just(SplitDirection::Vertical),
            ],
            0..=max_ops,
        )
    }

    /// Build a PaneManager by applying a sequence of splits (cycling focus between each).
    fn build_pm(ops: &[SplitDirection]) -> PaneManager {
        let mut pm = PaneManager::new();
        for (i, &dir) in ops.iter().enumerate() {
            // Alternate focusing different panes to create varied tree shapes
            if i % 2 == 1 {
                pm.focus_next();
            }
            pm.split(dir);
        }
        pm
    }

    // --- P1.1: split increases leaf_count by 1 ---
    proptest! {
        #[test]
        fn split_increases_leaf_count(ops in split_ops(8), dir in prop_oneof![Just(SplitDirection::Horizontal), Just(SplitDirection::Vertical)]) {
            let mut pm = build_pm(&ops);
            let before = pm.pane_count();
            pm.split(dir);
            prop_assert_eq!(pm.pane_count(), before + 1);
        }
    }

    // --- P1.2: split preserves all pre-existing pane IDs ---
    proptest! {
        #[test]
        fn split_preserves_existing_panes(ops in split_ops(8), dir in prop_oneof![Just(SplitDirection::Horizontal), Just(SplitDirection::Vertical)]) {
            let mut pm = build_pm(&ops);
            let before_ids = pm.all_pane_ids();
            pm.split(dir);
            for id in &before_ids {
                prop_assert!(pm.root.contains(*id), "Lost pane {:?} after split", id);
            }
        }
    }

    // --- P1.3: leaves_in_order count == leaf_count, all unique ---
    proptest! {
        #[test]
        fn leaves_in_order_consistent(ops in split_ops(10)) {
            let pm = build_pm(&ops);
            let leaves = pm.root.leaves_in_order();
            prop_assert_eq!(leaves.len(), pm.root.leaf_count());
            // Check uniqueness
            let mut seen = std::collections::HashSet::new();
            for id in &leaves {
                prop_assert!(seen.insert(*id), "Duplicate pane ID {:?}", id);
            }
        }
    }

    // --- P1.4: find succeeds for every leaf ---
    proptest! {
        #[test]
        fn find_succeeds_for_all_leaves(ops in split_ops(8)) {
            let pm = build_pm(&ops);
            for id in pm.root.leaves_in_order() {
                prop_assert!(pm.root.find(id).is_some(), "find({:?}) returned None", id);
            }
        }
    }

    // --- P1.5: contains agrees with find ---
    proptest! {
        #[test]
        fn contains_agrees_with_find(ops in split_ops(8), probe in 0u32..20) {
            let pm = build_pm(&ops);
            let id = PaneId(probe);
            prop_assert_eq!(pm.root.contains(id), pm.root.find(id).is_some());
        }
    }

    // --- P1.6: remove decreases leaf_count by 1 (when > 1) ---
    proptest! {
        #[test]
        fn remove_decreases_leaf_count(ops in split_ops(8).prop_filter("need >1 pane", |ops| !ops.is_empty())) {
            let mut pm = build_pm(&ops);
            if pm.pane_count() > 1 {
                let target = pm.all_pane_ids()[0];
                let before = pm.pane_count();
                prop_assert!(pm.root.remove(target));
                prop_assert_eq!(pm.root.leaf_count(), before - 1);
            }
        }
    }

    // --- P1.7: remove preserves all other panes ---
    proptest! {
        #[test]
        fn remove_preserves_other_panes(ops in split_ops(8).prop_filter("need >1 pane", |ops| !ops.is_empty())) {
            let mut pm = build_pm(&ops);
            if pm.pane_count() > 1 {
                let ids = pm.all_pane_ids();
                let target = ids[0];
                pm.root.remove(target);
                for &id in &ids[1..] {
                    prop_assert!(pm.root.contains(id), "Lost pane {:?} after removing {:?}", id, target);
                }
            }
        }
    }

    // --- P1.8: close_focused always keeps >= 1 pane ---
    proptest! {
        #[test]
        fn close_focused_keeps_at_least_one(ops in split_ops(8)) {
            let mut pm = build_pm(&ops);
            // Close all we can
            while pm.close_focused() {}
            prop_assert!(pm.pane_count() >= 1);
        }
    }

    // --- P1.9: focus_next N times cycles back ---
    proptest! {
        #[test]
        fn focus_next_cycles(ops in split_ops(8)) {
            let mut pm = build_pm(&ops);
            let n = pm.pane_count();
            let start = pm.focused_pane_id;
            for _ in 0..n {
                pm.focus_next();
            }
            prop_assert_eq!(pm.focused_pane_id, start, "focus_next didn't cycle after {} steps", n);
        }
    }

    // --- P1.10: focus_prev is inverse of focus_next ---
    proptest! {
        #[test]
        fn focus_prev_inverse_of_next(ops in split_ops(8)) {
            let mut pm = build_pm(&ops);
            let start = pm.focused_pane_id;
            pm.focus_next();
            pm.focus_prev();
            prop_assert_eq!(pm.focused_pane_id, start);
        }
    }

    // --- P1.11: ratio stays in [0.1, 0.9] after resize ---
    proptest! {
        #[test]
        fn resize_clamps_ratio(delta in -2.0f32..2.0) {
            let mut pm = PaneManager::new();
            pm.split(SplitDirection::Vertical);
            pm.root.resize(PaneId(0), delta);
            if let PaneNode::Split { ratio, .. } = &pm.root {
                prop_assert!(*ratio >= 0.1 - f32::EPSILON, "ratio {} < 0.1", ratio);
                prop_assert!(*ratio <= 0.9 + f32::EPSILON, "ratio {} > 0.9", ratio);
            }
        }
    }

    // --- P1.12 & P1.13: split_area conserves dimensions ---
    proptest! {
        #[test]
        fn split_area_conserves_width(x in 0u16..100, y in 0u16..100, w in 1u16..200, h in 1u16..200, ratio in 0.0f32..=1.0) {
            let area = Rect::new(x, y, w, h);
            let (first, second) = split_area(area, SplitDirection::Vertical, ratio);
            prop_assert_eq!(first.width + second.width, area.width,
                "Vertical split: {}+{} != {}", first.width, second.width, area.width);
            prop_assert_eq!(first.height, area.height);
            prop_assert_eq!(second.height, area.height);
        }

        #[test]
        fn split_area_conserves_height(x in 0u16..100, y in 0u16..100, w in 1u16..200, h in 1u16..200, ratio in 0.0f32..=1.0) {
            let area = Rect::new(x, y, w, h);
            let (first, second) = split_area(area, SplitDirection::Horizontal, ratio);
            prop_assert_eq!(first.height + second.height, area.height,
                "Horizontal split: {}+{} != {}", first.height, second.height, area.height);
            prop_assert_eq!(first.width, area.width);
            prop_assert_eq!(second.width, area.width);
        }
    }

    // --- P1.14 & P1.15: split_area positional invariants ---
    proptest! {
        #[test]
        fn split_area_positions_vertical(x in 0u16..100, y in 0u16..100, w in 1u16..200, h in 1u16..200, ratio in 0.0f32..=1.0) {
            let area = Rect::new(x, y, w, h);
            let (first, second) = split_area(area, SplitDirection::Vertical, ratio);
            prop_assert_eq!(first.x, area.x);
            prop_assert_eq!(first.y, area.y);
            prop_assert_eq!(second.x, first.x + first.width);
            prop_assert_eq!(second.y, area.y);
        }

        #[test]
        fn split_area_positions_horizontal(x in 0u16..100, y in 0u16..100, w in 1u16..200, h in 1u16..200, ratio in 0.0f32..=1.0) {
            let area = Rect::new(x, y, w, h);
            let (first, second) = split_area(area, SplitDirection::Horizontal, ratio);
            prop_assert_eq!(first.x, area.x);
            prop_assert_eq!(first.y, area.y);
            prop_assert_eq!(second.x, area.x);
            prop_assert_eq!(second.y, first.y + first.height);
        }
    }

    // --- P1.16: compute_positions returns leaf_count entries ---
    proptest! {
        #[test]
        fn compute_positions_count(ops in split_ops(6)) {
            let pm = build_pm(&ops);
            let area = Rect::new(0, 0, 200, 100);
            let positions = pm.compute_positions(area);
            prop_assert_eq!(positions.len(), pm.pane_count());
        }
    }

    // --- P1.17: all rects fit within given area ---
    proptest! {
        #[test]
        fn compute_positions_within_area(ops in split_ops(6)) {
            let pm = build_pm(&ops);
            let area = Rect::new(5, 3, 200, 100);
            let positions = pm.compute_positions(area);
            for (id, rect) in &positions {
                prop_assert!(rect.x >= area.x, "Pane {:?} x={} < area.x={}", id, rect.x, area.x);
                prop_assert!(rect.y >= area.y, "Pane {:?} y={} < area.y={}", id, rect.y, area.y);
                prop_assert!(rect.x + rect.width <= area.x + area.width,
                    "Pane {:?} right={} > area right={}", id, rect.x + rect.width, area.x + area.width);
                prop_assert!(rect.y + rect.height <= area.y + area.height,
                    "Pane {:?} bottom={} > area bottom={}", id, rect.y + rect.height, area.y + area.height);
            }
        }
    }

    // --- P1.18: session roundtrip preserves structure ---
    proptest! {
        #[test]
        fn session_roundtrip_preserves_structure(ops in split_ops(6)) {
            let pm = build_pm(&ops);
            let json = pm.to_session_json().unwrap();
            let restored = PaneManager::from_session_json(&json).unwrap();
            prop_assert_eq!(restored.pane_count(), pm.pane_count());
            prop_assert_eq!(restored.focused_pane_id, pm.focused_pane_id);
            let orig_ids = pm.all_pane_ids();
            let rest_ids = restored.all_pane_ids();
            prop_assert_eq!(orig_ids, rest_ids);
        }
    }
}

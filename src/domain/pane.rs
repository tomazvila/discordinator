use super::types::{
    ChannelMarker, GuildMarker, Id, InputState, PaneId, ScrollState, SplitDirection,
};

/// A pane leaf — an independent channel view.
#[derive(Debug, Clone)]
pub struct Pane {
    pub id: PaneId,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub scroll: ScrollState,
    pub input: InputState,
}

impl Pane {
    pub fn new(id: PaneId) -> Self {
        Self {
            id,
            channel_id: None,
            guild_id: None,
            scroll: ScrollState::Following,
            input: InputState::default(),
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
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    /// Find a leaf by PaneId.
    pub fn find(&self, id: PaneId) -> Option<&Pane> {
        match self {
            PaneNode::Leaf(pane) => {
                if pane.id == id {
                    Some(pane)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                first.find(id).or_else(|| second.find(id))
            }
        }
    }

    /// Find a mutable leaf by PaneId.
    pub fn find_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            PaneNode::Leaf(pane) => {
                if pane.id == id {
                    Some(pane)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
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
            PaneNode::Leaf(pane) => result.push(pane.id),
            PaneNode::Split { first, second, .. } => {
                first.collect_leaves(result);
                second.collect_leaves(result);
            }
        }
    }

    /// Count total leaf panes.
    pub fn leaf_count(&self) -> usize {
        match self {
            PaneNode::Leaf(_) => 1,
            PaneNode::Split { first, second, .. } => {
                first.leaf_count() + second.leaf_count()
            }
        }
    }

    /// Check if a pane with the given ID exists in this tree.
    pub fn contains(&self, id: PaneId) -> bool {
        self.find(id).is_some()
    }

    /// Split a leaf pane into two. The original pane stays in `first`,
    /// a new pane is created in `second`. Returns the new pane's ID.
    /// Returns None if the pane is not found.
    pub fn split(&mut self, target_id: PaneId, direction: SplitDirection, new_id: PaneId) -> bool {
        match self {
            PaneNode::Leaf(pane) => {
                if pane.id == target_id {
                    let original = std::mem::replace(
                        self,
                        PaneNode::Leaf(Pane::new(PaneId(0))), // placeholder
                    );
                    *self = PaneNode::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(original),
                        second: Box::new(PaneNode::Leaf(Pane::new(new_id))),
                    };
                    true
                } else {
                    false
                }
            }
            PaneNode::Split { first, second, .. } => {
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
        if let PaneNode::Leaf(_) = self {
            return false;
        }
        self.remove_inner(target_id)
    }

    fn remove_inner(&mut self, target_id: PaneId) -> bool {
        match self {
            PaneNode::Leaf(_) => false,
            PaneNode::Split { first, second, .. } => {
                // Check if first child is the target leaf
                if let PaneNode::Leaf(pane) = first.as_ref() {
                    if pane.id == target_id {
                        // Promote second to take our place
                        *self = *second.clone();
                        return true;
                    }
                }
                // Check if second child is the target leaf
                if let PaneNode::Leaf(pane) = second.as_ref() {
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
            PaneNode::Leaf(_) => false,
            PaneNode::Split {
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
        let leaves = self.root.leaves_in_order();
        // Find next pane to focus
        let current_idx = leaves.iter().position(|&id| id == self.focused_pane_id);
        let removed = self.root.remove(self.focused_pane_id);
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
            // Clear zoom if zoomed pane was closed
            if self.zoom_state == Some(self.focused_pane_id) {
                // Still valid
            } else if let Some(zoom_id) = self.zoom_state {
                if !self.root.contains(zoom_id) {
                    self.zoom_state = None;
                }
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
}

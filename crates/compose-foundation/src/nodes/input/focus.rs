//! Focus management system that integrates with modifier chains.
//!
//! This module provides the focus manager that tracks the currently focused
//! node and handles focus transitions using capability-aware traversal through
//! modifier chains.

use std::collections::HashMap;

/// Unique identifier for focusable nodes.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusId(pub(crate) usize);

impl FocusId {
    pub fn new(id: usize) -> Self {
        Self(id)
    }

    pub fn as_usize(self) -> usize {
        self.0
    }
}

/// Focus manager that tracks and manages focus state across the composition.
///
/// This mirrors Jetpack Compose's FocusManager and uses capability-filtered
/// traversal to find and navigate between focus targets.
pub struct FocusManager {
    /// The currently focused node ID, if any.
    active_focus_id: Option<FocusId>,
    /// Map of focus IDs to their focus state.
    focus_states: HashMap<FocusId, crate::modifier::FocusState>,
    /// Next ID to allocate for new focus targets.
    next_id: usize,
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusManager {
    pub fn new() -> Self {
        Self {
            active_focus_id: None,
            focus_states: HashMap::new(),
            next_id: 1,
        }
    }

    /// Allocates a new unique focus ID.
    pub fn allocate_focus_id(&mut self) -> FocusId {
        let id = FocusId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Returns the currently focused node ID.
    pub fn active_focus_id(&self) -> Option<FocusId> {
        self.active_focus_id
    }

    /// Requests focus for the given node.
    ///
    /// This clears focus from the previously focused node (if any) and
    /// sets the given node as active.
    pub fn request_focus(&mut self, id: FocusId) -> bool {
        // Clear previous focus
        if let Some(prev_id) = self.active_focus_id {
            if prev_id != id {
                self.focus_states
                    .insert(prev_id, crate::modifier::FocusState::Inactive);
            }
        }

        // Set new focus
        self.active_focus_id = Some(id);
        self.focus_states
            .insert(id, crate::modifier::FocusState::Active);
        true
    }

    /// Clears focus from the currently focused node.
    pub fn clear_focus(&mut self) {
        if let Some(id) = self.active_focus_id.take() {
            self.focus_states
                .insert(id, crate::modifier::FocusState::Inactive);
        }
    }

    /// Captures focus, preventing other nodes from taking focus.
    pub fn capture_focus(&mut self) -> bool {
        if let Some(id) = self.active_focus_id {
            self.focus_states
                .insert(id, crate::modifier::FocusState::Captured);
            true
        } else {
            false
        }
    }

    /// Releases captured focus.
    pub fn free_focus(&mut self) -> bool {
        if let Some(id) = self.active_focus_id {
            let state = self.focus_states.get(&id);
            if matches!(state, Some(crate::modifier::FocusState::Captured)) {
                self.focus_states
                    .insert(id, crate::modifier::FocusState::Active);
                return true;
            }
        }
        false
    }

    /// Gets the focus state for a given node.
    pub fn focus_state(&self, id: FocusId) -> crate::modifier::FocusState {
        self.focus_states
            .get(&id)
            .copied()
            .unwrap_or(crate::modifier::FocusState::Inactive)
    }

    /// Returns whether the given node is currently focused.
    pub fn is_focused(&self, id: FocusId) -> bool {
        self.active_focus_id == Some(id)
    }
}

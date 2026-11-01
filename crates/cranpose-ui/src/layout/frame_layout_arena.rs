use std::cell::RefCell;

use cranpose_core::NodeId;
use cranpose_ui_layout::Placement;

// The scratch lists of one measure, taken from the arena and given back
// together: one pop and one push a measure instead of one for each list.
#[derive(Default)]
struct MeasureLists {
    child_ids: Vec<NodeId>,
    placements: Vec<Placement>,
    placement_indices: Vec<usize>,
}

#[derive(Default)]
pub(crate) struct FrameLayoutArena {
    // The lists of the measures that ran, for the next ones to take.
    lists: Vec<MeasureLists>,
    // The nodes the dirty bubbles started from, which a pass takes.
    pub(super) dirty_roots: Vec<NodeId>,
    /// The marks a pass that starts from the dirty nodes cleared.
    pub(super) cleared_marks: Vec<(NodeId, crate::widgets::nodes::LayoutDirt)>,
}

#[cfg(test)]
impl FrameLayoutArena {
    pub(crate) fn available_placement_scratch_count(&self) -> usize {
        self.lists.len()
    }

    pub(crate) fn seed_placement_scratch_for_test(&mut self) {
        self.lists.push(MeasureLists {
            placements: Vec::with_capacity(1),
            ..MeasureLists::default()
        });
    }
}

pub(super) struct VecPools<'a> {
    arena: &'a RefCell<FrameLayoutArena>,
    pub(super) child_ids: Vec<NodeId>,
    pub(super) placements: Vec<Placement>,
    pub(super) placement_indices: Vec<usize>,
}

impl<'a> VecPools<'a> {
    pub(super) fn acquire(arena: &'a RefCell<FrameLayoutArena>) -> Self {
        let MeasureLists {
            child_ids,
            placements,
            placement_indices,
        } = arena.borrow_mut().lists.pop().unwrap_or_default();
        Self {
            arena,
            child_ids,
            placements,
            placement_indices,
        }
    }
}

impl Drop for VecPools<'_> {
    fn drop(&mut self) {
        let mut lists = MeasureLists {
            child_ids: std::mem::take(&mut self.child_ids),
            placements: std::mem::take(&mut self.placements),
            placement_indices: std::mem::take(&mut self.placement_indices),
        };
        lists.child_ids.clear();
        lists.placements.clear();
        lists.placement_indices.clear();
        self.arena.borrow_mut().lists.push(lists);
    }
}

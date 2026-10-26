use std::cell::RefCell;

use cranpose_core::NodeId;
use cranpose_ui_layout::Placement;

pub(super) struct ScratchVecPool<T> {
    available: Vec<Vec<T>>,
}

impl<T> ScratchVecPool<T> {
    pub(super) fn acquire(&mut self) -> Vec<T> {
        self.available.pop().unwrap_or_default()
    }

    pub(super) fn release(&mut self, mut values: Vec<T>) {
        values.clear();
        self.available.push(values);
    }

    #[cfg(test)]
    pub(super) fn available_count(&self) -> usize {
        self.available.len()
    }
}

impl<T> Default for ScratchVecPool<T> {
    fn default() -> Self {
        Self {
            available: Vec::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct FrameLayoutArena {
    pub(super) tmp_child_ids: ScratchVecPool<NodeId>,
    pub(super) tmp_placements: ScratchVecPool<Placement>,
    pub(super) tmp_placement_indices: ScratchVecPool<usize>,
    /// The marks a pass that starts from the dirty nodes cleared.
    pub(super) cleared_marks: Vec<(NodeId, crate::widgets::nodes::LayoutDirt)>,
}

#[cfg(test)]
impl FrameLayoutArena {
    pub(crate) fn available_placement_scratch_count(&self) -> usize {
        self.tmp_placements.available_count()
    }

    pub(crate) fn seed_placement_scratch_for_test(&mut self) {
        self.tmp_placements.release(Vec::with_capacity(1));
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
        let mut pools = arena.borrow_mut();
        let child_ids = pools.tmp_child_ids.acquire();
        let placements = pools.tmp_placements.acquire();
        let placement_indices = pools.tmp_placement_indices.acquire();
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
        let mut pools = self.arena.borrow_mut();
        pools
            .tmp_child_ids
            .release(std::mem::take(&mut self.child_ids));
        pools
            .tmp_placements
            .release(std::mem::take(&mut self.placements));
        pools
            .tmp_placement_indices
            .release(std::mem::take(&mut self.placement_indices));
    }
}

use crate::collections::map::HashMap;
use crate::AnchorId;
use std::cell::Cell;

use super::{Slot, SlotTableDebugStats};

const INVALID_ANCHOR_POS: usize = usize::MAX;

pub(crate) struct AnchorMap {
    positions: HashMap<usize, usize>,
    dirty: bool,
    next_anchor_id: Cell<usize>,
    free_anchor_ids: Vec<usize>,
}

impl Default for AnchorMap {
    fn default() -> Self {
        Self {
            positions: HashMap::default(),
            dirty: false,
            next_anchor_id: Cell::new(1),
            free_anchor_ids: Vec::new(),
        }
    }
}

impl AnchorMap {
    pub(crate) fn debug_heap_bytes(&self) -> usize {
        let anchors_bytes = self.positions.capacity() * std::mem::size_of::<(usize, usize)>();
        let free_anchors_bytes = self.free_anchor_ids.capacity() * std::mem::size_of::<usize>();
        anchors_bytes + free_anchors_bytes
    }

    pub(crate) fn fill_debug_stats(&self, stats: &mut SlotTableDebugStats) {
        stats.anchors_len = self.positions.len();
        stats.anchors_cap = self.positions.capacity();
        stats.gap_metadata_len = 0;
        stats.gap_metadata_cap = 0;
        stats.free_anchor_ids_len = self.free_anchor_ids.len();
        stats.free_anchor_ids_cap = self.free_anchor_ids.capacity();
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(crate) fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub(crate) fn shift_positions_from(&mut self, start_slot: usize, delta: isize) {
        for pos in self.positions.values_mut() {
            if *pos >= start_slot {
                *pos = ((*pos as isize) + delta) as usize;
            }
        }
    }

    pub(crate) fn allocate_anchor(&mut self) -> AnchorId {
        if let Some(id) = self.free_anchor_ids.pop() {
            return AnchorId(id);
        }

        let id = self.next_anchor_id.get();
        self.next_anchor_id.set(id + 1);
        AnchorId(id)
    }

    pub(crate) fn free_anchor(&mut self, anchor: AnchorId) {
        if anchor.is_valid() && anchor.0 != 0 {
            self.positions.remove(&anchor.0);
            self.free_anchor_ids.push(anchor.0);
        }
    }

    pub(crate) fn remove_position(&mut self, anchor: AnchorId) {
        if anchor.is_valid() && anchor.0 != 0 {
            self.positions.remove(&anchor.0);
        }
    }

    pub(crate) fn register_anchor(&mut self, anchor: AnchorId, position: usize) {
        debug_assert!(anchor.is_valid(), "attempted to register invalid anchor");
        if anchor.0 == 0 {
            return;
        }
        self.positions.insert(anchor.0, position);
    }

    pub(crate) fn resolve_anchor(&self, anchor: AnchorId) -> Option<usize> {
        if anchor.0 == 0 {
            return None;
        }
        self.positions.get(&anchor.0).copied()
    }

    pub(crate) fn rebuild_all_positions(&mut self, slots: &[Slot]) {
        let live_anchor_count = slots
            .iter()
            .filter(|slot| slot.anchor_id().is_valid())
            .count();
        let mut positions = HashMap::default();
        positions.reserve(live_anchor_count);
        for slot in slots {
            let idx = slot.anchor_id().0;
            if idx != 0 {
                positions.insert(idx, INVALID_ANCHOR_POS);
            }
        }

        for (position, slot) in slots.iter().enumerate() {
            let idx = slot.anchor_id().0;
            if idx == 0 {
                continue;
            }
            positions.insert(idx, position);
        }
        self.positions = positions;
    }

    pub(crate) fn rebuild_positions(&mut self, slots: &[Slot]) {
        let live_anchor_count = slots
            .iter()
            .filter(|slot| slot.anchor_id().is_valid())
            .count();
        let mut positions = HashMap::default();
        positions.reserve(live_anchor_count);
        for (position, slot) in slots.iter().enumerate() {
            let anchor = slot.anchor_id();
            if anchor.is_valid() {
                positions.insert(anchor.0, position);
            }
        }
        self.positions = positions;
        self.free_anchor_ids = Vec::new();
        self.next_anchor_id.set(
            self.next_anchor_id.get().max(
                self.positions
                    .keys()
                    .copied()
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1),
            ),
        );
    }
}

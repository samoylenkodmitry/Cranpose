use crate::collections::map::HashMap;
use crate::{AnchorId, Key, NodeId};
use std::cell::Cell;

use super::{unpack_slot_len, PackedScopeId, Slot, SlotTableDebugStats};

const INVALID_ANCHOR_POS: usize = usize::MAX;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GapMetadata {
    pub(super) group_key: Option<Key>,
    pub(super) boundary_key: Option<Key>,
    pub(super) group_scope: PackedScopeId,
    pub(super) group_len: u32,
    pub(super) preserved_node: Option<(NodeId, u32)>,
}

pub(crate) struct AnchorRegistry {
    positions: HashMap<usize, usize>,
    gap_metadata: HashMap<usize, GapMetadata>,
    dirty: bool,
    next_anchor_id: Cell<usize>,
    free_anchor_ids: Vec<usize>,
}

impl Default for AnchorRegistry {
    fn default() -> Self {
        Self {
            positions: HashMap::default(),
            gap_metadata: HashMap::default(),
            dirty: false,
            next_anchor_id: Cell::new(1),
            free_anchor_ids: Vec::new(),
        }
    }
}

impl AnchorRegistry {
    pub(crate) fn debug_heap_bytes(&self) -> usize {
        let anchors_bytes = self.positions.capacity() * std::mem::size_of::<(usize, usize)>();
        let gap_metadata_bytes =
            self.gap_metadata.capacity() * std::mem::size_of::<(usize, GapMetadata)>();
        let free_anchors_bytes = self.free_anchor_ids.capacity() * std::mem::size_of::<usize>();
        anchors_bytes + gap_metadata_bytes + free_anchors_bytes
    }

    pub(crate) fn fill_debug_stats(&self, stats: &mut SlotTableDebugStats) {
        stats.anchors_len = self.positions.len();
        stats.anchors_cap = self.positions.capacity();
        stats.gap_metadata_len = self.gap_metadata.len();
        stats.gap_metadata_cap = self.gap_metadata.capacity();
        stats.free_anchor_ids_len = self.free_anchor_ids.len();
        stats.free_anchor_ids_cap = self.free_anchor_ids.capacity();
    }

    pub(crate) fn clear_gap_metadata_for_replacement(
        &mut self,
        slots: &[Slot],
        index: usize,
        new_slot: &Slot,
    ) {
        if matches!(new_slot, Slot::Gap { .. }) {
            return;
        }
        let old_anchor = match slots.get(index) {
            Some(Slot::Gap { anchor }) => Some(*anchor),
            _ => None,
        };
        if let Some(anchor) = old_anchor {
            self.clear_gap_metadata(anchor);
        }
    }

    pub(crate) fn gap_metadata_for_anchor(&self, anchor: AnchorId) -> Option<GapMetadata> {
        anchor
            .is_valid()
            .then(|| self.gap_metadata.get(&anchor.0).copied())
            .flatten()
    }

    pub(crate) fn set_gap_metadata(&mut self, anchor: AnchorId, metadata: GapMetadata) {
        if !anchor.is_valid() {
            return;
        }
        if metadata == GapMetadata::default() {
            self.gap_metadata.remove(&anchor.0);
        } else {
            self.gap_metadata.insert(anchor.0, metadata);
        }
    }

    pub(crate) fn clear_gap_metadata(&mut self, anchor: AnchorId) {
        if anchor.is_valid() {
            self.gap_metadata.remove(&anchor.0);
        }
    }

    pub(crate) fn gap_group_key(&self, anchor: AnchorId) -> Option<Key> {
        self.gap_metadata_for_anchor(anchor)
            .and_then(|metadata| metadata.group_key)
    }

    pub(crate) fn gap_boundary_key(&self, anchor: AnchorId) -> Option<Key> {
        self.gap_metadata_for_anchor(anchor)
            .and_then(|metadata| metadata.boundary_key)
    }

    pub(crate) fn gap_group_scope(&self, anchor: AnchorId) -> PackedScopeId {
        self.gap_metadata_for_anchor(anchor)
            .map(|metadata| metadata.group_scope)
            .unwrap_or_default()
    }

    pub(crate) fn gap_group_len(&self, anchor: AnchorId) -> u32 {
        self.gap_metadata_for_anchor(anchor)
            .map(|metadata| metadata.group_len)
            .unwrap_or(0)
    }

    pub(crate) fn gap_preserved_node(&self, anchor: AnchorId) -> Option<(NodeId, u32)> {
        self.gap_metadata_for_anchor(anchor)
            .and_then(|metadata| metadata.preserved_node)
    }

    pub(crate) fn gap_extent_for_anchor(&self, anchor: AnchorId) -> usize {
        unpack_slot_len(self.gap_group_len(anchor)).max(1)
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
            self.gap_metadata.remove(&anchor.0);
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

    pub(crate) fn clear_all_gap_metadata(&mut self) {
        self.gap_metadata = HashMap::default();
    }
}

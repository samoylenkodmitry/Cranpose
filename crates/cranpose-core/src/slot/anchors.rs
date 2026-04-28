#[cfg(any(test, debug_assertions))]
use super::SlotInvariantError;
use super::{dense_id_map::DenseIdMap, DetachedSubtree, GroupRecord, SlotTable};
#[cfg(any(test, debug_assertions))]
use crate::collections::map::HashSet;
use crate::{
    collections::map::HashMap, retention::RetentionManager, AnchorId, RecomposeScope, ScopeId,
};
use std::{cmp::Reverse, collections::BinaryHeap, mem};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnchorState {
    Active(usize),
    Detached,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AnchorSlot {
    generation: u32,
    state: AnchorState,
}

#[derive(Default)]
pub(crate) struct AnchorRegistry {
    dense_states: DenseIdMap<AnchorSlot>,
    sparse_states: HashMap<u32, AnchorSlot>,
    free_ids: BinaryHeap<Reverse<u32>>,
    reused_generations: HashMap<u32, u32>,
    next_anchor: usize,
    active_count: usize,
    detached_count: usize,
}

impl AnchorRegistry {
    const DENSE_STORAGE_ID_LIMIT: usize = 65_536;

    pub(super) fn new() -> Self {
        Self {
            dense_states: DenseIdMap::new(),
            sparse_states: HashMap::default(),
            free_ids: BinaryHeap::new(),
            reused_generations: HashMap::default(),
            next_anchor: 1,
            active_count: 0,
            detached_count: 0,
        }
    }

    pub(super) fn allocate(&mut self) -> AnchorId {
        let anchor = if let Some(Reverse(id)) = self.free_ids.pop() {
            AnchorId {
                id,
                generation: self.reused_generations.remove(&id).unwrap_or(2),
            }
        } else {
            let anchor = AnchorId::new(self.next_anchor);
            self.next_anchor = self
                .next_anchor
                .checked_add(1)
                .expect("anchor counter overflow");
            anchor
        };
        let replaced = self.set_state(anchor, AnchorState::Invalidated);
        debug_assert!(replaced.is_none(), "group anchors must stay unique");
        anchor
    }

    pub(super) fn state(&self, anchor: AnchorId) -> Option<AnchorState> {
        if !anchor.is_valid() {
            return None;
        }
        let slot = self.slot(anchor.id)?;
        (slot.generation == anchor.generation).then_some(slot.state)
    }

    pub(super) fn active_index(&self, anchor: AnchorId) -> Option<usize> {
        match self.state(anchor) {
            Some(AnchorState::Active(index)) => Some(index),
            _ => None,
        }
    }

    pub(super) fn active_len(&self) -> usize {
        self.active_count
    }

    pub(super) fn slot_len(&self) -> usize {
        self.dense_states.len() + self.sparse_states.len()
    }

    pub(super) fn sparse_slot_len(&self) -> usize {
        let reserved_zero_slot = usize::from(self.dense_states.storage_len() > 0);
        self.dense_states
            .storage_len()
            .saturating_sub(self.dense_states.len() + reserved_zero_slot)
    }

    pub(super) fn detached_len(&self) -> usize {
        self.detached_count
    }

    pub(super) fn invalidated_len(&self) -> usize {
        self.slot_len()
            .saturating_sub(self.active_count + self.detached_count)
    }

    pub(super) fn free_len(&self) -> usize {
        self.free_ids.len()
    }

    pub(super) fn active_entries(&self) -> impl Iterator<Item = (AnchorId, usize)> + '_ {
        self.dense_states
            .iter()
            .map(|(id, slot)| {
                (
                    u32::try_from(id).expect("dense anchor state index must fit u32"),
                    slot,
                )
            })
            .chain(self.sparse_states.iter().map(|(&id, slot)| (id, slot)))
            .filter_map(|(id, slot)| match slot.state {
                AnchorState::Active(group_index) => Some((
                    AnchorId {
                        id,
                        generation: slot.generation,
                    },
                    group_index,
                )),
                AnchorState::Detached | AnchorState::Invalidated => None,
            })
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn validate_integrity(&self) -> Result<(), SlotInvariantError> {
        let mut active_count = 0usize;
        let mut detached_count = 0usize;

        for (id, slot) in self.anchor_slots() {
            if id == 0 || id as usize >= self.next_anchor {
                return Err(SlotInvariantError::AnchorRegistryInternalMismatch {
                    detail: "registered anchor id must be allocated",
                    anchor_id: Some(id),
                    expected: self.next_anchor,
                    actual: id as usize,
                });
            }
            match slot.state {
                AnchorState::Active(_) => active_count += 1,
                AnchorState::Detached => detached_count += 1,
                AnchorState::Invalidated => {}
            }
        }

        self.validate_state_count("active", self.active_count, active_count)?;
        self.validate_state_count("detached", self.detached_count, detached_count)?;

        let mut free_ids = HashSet::default();
        for Reverse(id) in self.free_ids.iter().copied() {
            if !free_ids.insert(id) {
                return Err(SlotInvariantError::AnchorRegistryInternalMismatch {
                    detail: "free anchor id must be unique",
                    anchor_id: Some(id),
                    expected: 1,
                    actual: 2,
                });
            }
            if matches!(
                self.slot(id).map(|slot| slot.state),
                Some(AnchorState::Active(_) | AnchorState::Detached)
            ) {
                return Err(SlotInvariantError::AnchorRegistryInternalMismatch {
                    detail: "free anchor id must not be active or detached",
                    anchor_id: Some(id),
                    expected: 0,
                    actual: 1,
                });
            }
        }

        for &id in self.reused_generations.keys() {
            if !free_ids.contains(&id) {
                return Err(SlotInvariantError::AnchorRegistryInternalMismatch {
                    detail: "reused anchor generation must belong to a free id",
                    anchor_id: Some(id),
                    expected: 1,
                    actual: 0,
                });
            }
        }

        Ok(())
    }

    pub(super) fn capacity(&self) -> usize {
        self.dense_states.capacity() + self.sparse_states.capacity()
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.dense_states.capacity() * mem::size_of::<Option<AnchorSlot>>()
            + self.sparse_states.capacity() * mem::size_of::<(u32, AnchorSlot)>()
            + self.free_ids.capacity() * mem::size_of::<Reverse<u32>>()
            + self.reused_generations.capacity() * mem::size_of::<(u32, u32)>()
    }

    pub(super) fn set_active(&mut self, anchor: AnchorId, group_index: usize) {
        self.set_state(anchor, AnchorState::Active(group_index));
    }

    pub(super) fn mark_detached(&mut self, anchor: AnchorId) {
        if anchor.is_valid() {
            self.set_state(anchor, AnchorState::Detached);
        }
    }

    pub(super) fn mark_detached_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            self.mark_detached(group.anchor);
        }
    }

    pub(super) fn clear(&mut self) {
        self.dense_states.clear();
        self.sparse_states.clear();
        self.free_ids.clear();
        self.reused_generations.clear();
        self.next_anchor = 1;
        self.active_count = 0;
        self.detached_count = 0;
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.dense_states.shrink_to_fit();
        self.sparse_states.shrink_to_fit();
        self.free_ids.shrink_to_fit();
        self.reused_generations.shrink_to_fit();
    }

    fn invalidate_state(&mut self, anchor: AnchorId) -> bool {
        if !anchor.is_valid() {
            return false;
        }
        let Some(slot) = self.slot(anchor.id) else {
            return false;
        };
        if slot.generation != anchor.generation {
            return false;
        }
        let removed = self.remove_slot(anchor.id);
        let removed = removed.expect("validated anchor state must remove");
        self.adjust_state_counts(Some(removed.state), None);
        self.enqueue_reuse(anchor);
        true
    }

    fn set_state(&mut self, anchor: AnchorId, state: AnchorState) -> Option<AnchorState> {
        if !anchor.is_valid() {
            return None;
        }
        if let Some(slot) = self.slot(anchor.id) {
            if slot.generation != anchor.generation {
                return None;
            }
        }
        let previous = self.insert_slot(anchor.id, anchor.generation, state);
        self.adjust_state_counts(previous.map(|slot| slot.state), Some(state));
        previous.map(|slot| slot.state)
    }

    fn slot(&self, id: u32) -> Option<&AnchorSlot> {
        if Self::uses_dense_storage(id) {
            self.dense_states.get(id as usize)
        } else {
            self.sparse_states.get(&id)
        }
    }

    #[cfg(any(test, debug_assertions))]
    fn anchor_slots(&self) -> impl Iterator<Item = (u32, &AnchorSlot)> + '_ {
        self.dense_states
            .iter()
            .map(|(id, slot)| {
                (
                    u32::try_from(id).expect("dense anchor state index must fit u32"),
                    slot,
                )
            })
            .chain(self.sparse_states.iter().map(|(&id, slot)| (id, slot)))
    }

    fn insert_slot(&mut self, id: u32, generation: u32, state: AnchorState) -> Option<AnchorSlot> {
        let slot = AnchorSlot { generation, state };
        if Self::uses_dense_storage(id) {
            self.dense_states.insert(id as usize, slot)
        } else {
            self.sparse_states.insert(id, slot)
        }
    }

    fn remove_slot(&mut self, id: u32) -> Option<AnchorSlot> {
        if Self::uses_dense_storage(id) {
            self.dense_states.remove(id as usize)
        } else {
            self.sparse_states.remove(&id)
        }
    }

    fn uses_dense_storage(id: u32) -> bool {
        id as usize <= Self::DENSE_STORAGE_ID_LIMIT
    }

    fn adjust_state_counts(&mut self, previous: Option<AnchorState>, next: Option<AnchorState>) {
        if matches!(previous, Some(AnchorState::Active(_))) {
            self.active_count -= 1;
        }
        if matches!(previous, Some(AnchorState::Detached)) {
            self.detached_count -= 1;
        }
        if matches!(next, Some(AnchorState::Active(_))) {
            self.active_count += 1;
        }
        if matches!(next, Some(AnchorState::Detached)) {
            self.detached_count += 1;
        }
    }

    fn enqueue_reuse(&mut self, anchor: AnchorId) {
        self.free_ids.push(Reverse(anchor.id));
        let next_generation = anchor
            .generation
            .checked_add(1)
            .expect("anchor generation counter overflow");
        if next_generation == 2 {
            self.reused_generations.remove(&anchor.id);
            return;
        }
        self.reused_generations.insert(anchor.id, next_generation);
    }

    fn maybe_shrink_sparse_storage(&mut self) {
        if self.capacity() <= Self::DENSE_STORAGE_ID_LIMIT {
            return;
        }
        if self.slot_len().saturating_mul(4) >= self.capacity() {
            return;
        }
        self.dense_states.shrink_to_fit();
        self.sparse_states.shrink_to_fit();
        self.free_ids.shrink_to_fit();
        self.reused_generations.shrink_to_fit();
    }

    #[cfg(any(test, debug_assertions))]
    fn validate_state_count(
        &self,
        detail: &'static str,
        expected: usize,
        actual: usize,
    ) -> Result<(), SlotInvariantError> {
        if expected == actual {
            return Ok(());
        }
        Err(SlotInvariantError::AnchorRegistryInternalMismatch {
            detail,
            anchor_id: None,
            expected,
            actual,
        })
    }
}

impl SlotTable {
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn anchor_state(&self, anchor: AnchorId) -> Option<AnchorState> {
        self.anchors.state(anchor)
    }

    pub(crate) fn invalidate_detached_subtree_anchors(&mut self, subtree: &DetachedSubtree) {
        let mut removed = false;
        for anchor in subtree.group_anchors() {
            removed |= self.anchors.invalidate_state(anchor);
        }
        self.invalidate_detached_subtree_payload_anchors(subtree);
        if removed {
            self.anchors.maybe_shrink_sparse_storage();
        }
    }

    pub(crate) fn compact_anchor_registry_storage(
        &mut self,
        retention: Option<&mut RetentionManager>,
        _scope_for_id: impl FnMut(ScopeId) -> Option<RecomposeScope>,
    ) {
        let retained_group_count = retention
            .as_ref()
            .map(|retention| {
                retention
                    .subtrees()
                    .map(DetachedSubtree::group_count)
                    .sum::<usize>()
            })
            .unwrap_or(0);
        let total_group_count = self.groups.len() + retained_group_count;
        if total_group_count == 0 {
            self.anchors.clear();
            return;
        }

        let max_anchor_id = self
            .groups
            .iter()
            .map(|group| group.anchor.id as usize)
            .chain(
                retention
                    .as_ref()
                    .into_iter()
                    .flat_map(|retention| retention.subtrees())
                    .flat_map(DetachedSubtree::group_anchors)
                    .map(|anchor| anchor.id as usize),
            )
            .max()
            .unwrap_or(0);
        let sparse_anchor_ids = max_anchor_id > total_group_count.max(256) * 4;
        let sparse_capacity = self.anchors.capacity() > total_group_count.max(256) * 8;
        if !sparse_anchor_ids && !sparse_capacity {
            return;
        }

        self.anchors.shrink_to_fit();
        self.payload_locations.clear();
        self.rebuild_payload_locations_for_group_range(super::GroupRange::new(
            0,
            self.groups.len(),
        ));
        self.recompute_scope_index();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_integrity_rejects_free_active_anchor_id() {
        let mut registry = AnchorRegistry::new();
        let anchor = registry.allocate();
        registry.set_active(anchor, 0);
        registry.free_ids.push(Reverse(anchor.id));

        assert_eq!(
            registry.validate_integrity(),
            Err(SlotInvariantError::AnchorRegistryInternalMismatch {
                detail: "free anchor id must not be active or detached",
                anchor_id: Some(anchor.id),
                expected: 0,
                actual: 1,
            })
        );
    }

    #[test]
    fn registry_integrity_rejects_reused_generation_without_free_id() {
        let mut registry = AnchorRegistry::new();
        let anchor = AnchorId {
            id: 7,
            generation: 2,
        };
        registry.reused_generations.insert(anchor.id, 3);

        assert_eq!(
            registry.validate_integrity(),
            Err(SlotInvariantError::AnchorRegistryInternalMismatch {
                detail: "reused anchor generation must belong to a free id",
                anchor_id: Some(anchor.id),
                expected: 1,
                actual: 0,
            })
        );
    }
}

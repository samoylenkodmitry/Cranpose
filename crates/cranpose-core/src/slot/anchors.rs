use std::{cmp::Reverse, collections::BinaryHeap, mem};

#[cfg(any(test, debug_assertions))]
use super::SlotInvariantError;
use super::{
    DetachedSubtree, GroupRecord, SlotTable,
    generational_registry::{GenerationalRegistryStorage, RegistryState},
};
#[cfg(any(test, debug_assertions))]
use crate::collections::map::HashSet;
use crate::{AnchorId, collections::map::HashMap, retention::RetentionManager};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnchorState {
    Active(usize),
    Detached,
    Invalidated,
}

impl RegistryState for AnchorState {
    fn is_active(self) -> bool {
        matches!(self, Self::Active(_))
    }

    fn is_detached(self) -> bool {
        matches!(self, Self::Detached)
    }
}

#[derive(Default)]
pub(crate) struct AnchorRegistry {
    storage: GenerationalRegistryStorage<AnchorState>,
    free_ids: BinaryHeap<Reverse<u32>>,
    reused_generations: HashMap<u32, u32>,
    next_anchor: usize,
}

impl AnchorRegistry {
    pub(super) fn new() -> Self {
        Self {
            storage: GenerationalRegistryStorage::new(),
            free_ids: BinaryHeap::new(),
            reused_generations: HashMap::default(),
            next_anchor: 1,
        }
    }

    #[cfg(test)]
    pub(super) fn allocate(&mut self) -> AnchorId {
        self.try_allocate().unwrap_or(AnchorId::INVALID)
    }

    pub(super) fn try_allocate(&mut self) -> Option<AnchorId> {
        let anchor = if let Some(Reverse(id)) = self.free_ids.pop() {
            AnchorId {
                id,
                generation: self.reused_generations.remove(&id).unwrap_or(2),
            }
        } else {
            if self.next_anchor > u32::MAX as usize {
                log::error!(
                    "slot table rejected group anchor allocation because id counter {} exceeds u32 storage",
                    self.next_anchor
                );
                return None;
            }
            let anchor = AnchorId::new(self.next_anchor);
            self.next_anchor = self.next_anchor.saturating_add(1);
            anchor
        };
        let replaced = self.set_state(anchor, AnchorState::Invalidated);
        debug_assert!(replaced.is_none(), "group anchors must stay unique");
        Some(anchor)
    }

    pub(super) fn state(&self, anchor: AnchorId) -> Option<AnchorState> {
        if !anchor.is_valid() {
            return None;
        }
        let slot = self.storage.slot(anchor.id as usize)?;
        (slot.generation == anchor.generation).then_some(slot.state)
    }

    pub(super) fn active_index(&self, anchor: AnchorId) -> Option<usize> {
        match self.state(anchor) {
            Some(AnchorState::Active(index)) => Some(index),
            _ => None,
        }
    }

    pub(super) fn is_detached(&self, anchor: AnchorId) -> bool {
        matches!(self.state(anchor), Some(AnchorState::Detached))
    }

    pub(super) fn active_len(&self) -> usize {
        self.storage.active_len()
    }

    #[cfg(test)]
    pub(in crate::slot) fn set_next_anchor_for_test(&mut self, next_anchor: usize) {
        self.next_anchor = next_anchor;
    }

    pub(super) fn slot_len(&self) -> usize {
        self.storage.slot_len()
    }

    pub(super) fn sparse_slot_len(&self) -> usize {
        self.storage.sparse_slot_len()
    }

    pub(super) fn detached_len(&self) -> usize {
        self.storage.detached_len()
    }

    pub(super) fn invalidated_len(&self) -> usize {
        self.storage.invalidated_len()
    }

    pub(super) fn free_len(&self) -> usize {
        self.free_ids.len()
    }

    pub(super) fn active_entries(&self) -> impl Iterator<Item = (AnchorId, usize)> + '_ {
        self.storage
            .slots()
            .filter_map(|(id, slot)| match slot.state {
                AnchorState::Active(group_index) => {
                    let id = u32::try_from(id).ok()?;
                    Some((
                        AnchorId {
                            id,
                            generation: slot.generation,
                        },
                        group_index,
                    ))
                }
                AnchorState::Detached | AnchorState::Invalidated => None,
            })
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn validate_integrity(&self) -> Result<(), SlotInvariantError> {
        let mut active_count = 0usize;
        let mut detached_count = 0usize;

        for (id, slot) in self.storage.slots() {
            if id == 0 || id >= self.next_anchor {
                return Err(SlotInvariantError::AnchorRegistryInternalMismatch {
                    detail: "registered anchor id must be allocated",
                    anchor_id: Some(u32::try_from(id).expect("anchor id must fit u32")),
                    expected: self.next_anchor,
                    actual: id,
                });
            }
            match slot.state {
                AnchorState::Active(_) => active_count += 1,
                AnchorState::Detached => detached_count += 1,
                AnchorState::Invalidated => {}
            }
        }

        self.validate_state_count("active", self.storage.active_len(), active_count)?;
        self.validate_state_count("detached", self.storage.detached_len(), detached_count)?;

        let mut free_ids: HashSet<u32> = HashSet::default();
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
                self.storage.slot(id as usize).map(|slot| slot.state),
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
        self.storage.capacity()
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.storage.heap_bytes()
            + self.free_ids.capacity() * mem::size_of::<Reverse<u32>>()
            + self.reused_generations.capacity() * mem::size_of::<(u32, u32)>()
    }

    pub(super) fn set_active(&mut self, anchor: AnchorId, group_index: usize) {
        let previous = self.set_state(anchor, AnchorState::Active(group_index));
        if previous.is_none() {
            log::error!("slot table ignored active-state update for stale group anchor {anchor:?}");
        }
    }

    pub(super) fn mark_detached(&mut self, anchor: AnchorId) {
        if anchor.is_valid() {
            let previous = self.set_state(anchor, AnchorState::Detached);
            if previous.is_none() {
                log::error!("slot table ignored detach for stale group anchor {anchor:?}");
            }
        }
    }

    pub(super) fn mark_detached_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            self.mark_detached(group.anchor);
        }
    }

    pub(super) fn clear(&mut self) {
        self.storage.clear();
        self.free_ids.clear();
        self.reused_generations.clear();
        self.next_anchor = 1;
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.storage.shrink_to_fit();
        self.free_ids.shrink_to_fit();
        self.reused_generations.shrink_to_fit();
    }

    fn invalidate_state(&mut self, anchor: AnchorId) -> bool {
        if !anchor.is_valid() {
            return false;
        }
        let Some(slot) = self.storage.slot(anchor.id as usize) else {
            return false;
        };
        if slot.generation != anchor.generation {
            return false;
        }
        if self.storage.remove_state(anchor.id as usize).is_none() {
            log::error!(
                "slot table ignored invalidation for group anchor removed during validation {anchor:?}"
            );
            return false;
        }
        self.enqueue_reuse(anchor);
        true
    }

    fn set_state(&mut self, anchor: AnchorId, state: AnchorState) -> Option<AnchorState> {
        if !anchor.is_valid() {
            return None;
        }
        self.storage
            .set_state(anchor.id as usize, anchor.generation, state)
    }

    fn enqueue_reuse(&mut self, anchor: AnchorId) {
        let Some(next_generation) = anchor.generation.checked_add(1) else {
            log::error!(
                "slot table cannot reuse group anchor {anchor:?}: generation counter overflow"
            );
            return;
        };
        self.free_ids.push(Reverse(anchor.id));
        if next_generation == 2 {
            self.reused_generations.remove(&anchor.id);
            return;
        }
        self.reused_generations.insert(anchor.id, next_generation);
    }

    fn maybe_shrink_sparse_storage(&mut self) {
        if self.storage.maybe_shrink_sparse_storage() {
            self.free_ids.shrink_to_fit();
            self.reused_generations.shrink_to_fit();
        }
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
    ) {
        let retained_group_count = retention.as_ref().map_or(0, |retention| {
            retention
                .subtrees()
                .map(DetachedSubtree::group_count)
                .sum::<usize>()
        });
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
    }
}

#[cfg(test)]
#[path = "tests/anchors_tests.rs"]
mod tests;

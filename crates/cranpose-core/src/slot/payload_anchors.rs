#[cfg(any(test, debug_assertions))]
use super::SlotInvariantError;
use super::{dense_id_map::DenseIdMap, DetachedSubtree, PayloadAnchor, PayloadRecord, SlotTable};
use crate::collections::map::HashMap;
use crate::AnchorId;
use std::{cmp::Reverse, collections::BinaryHeap, mem};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PayloadAnchorState {
    Active { owner: AnchorId, index: usize },
    Detached,
}

#[cfg(any(test, debug_assertions))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PayloadAnchorLifecycle {
    Active,
    Detached,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PayloadAnchorSlot {
    generation: u32,
    state: PayloadAnchorState,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FreePayloadAnchorIdRange {
    start: u32,
    end: u32,
}

impl FreePayloadAnchorIdRange {
    fn singleton(id: u32) -> Self {
        Self { start: id, end: id }
    }

    fn len(self) -> usize {
        self.end as usize - self.start as usize + 1
    }

    fn contains(self, id: u32) -> bool {
        self.start <= id && id <= self.end
    }

    fn pop_front(&mut self) -> u32 {
        let id = self.start;
        if self.start == self.end {
            self.start = 1;
            self.end = 0;
        } else {
            self.start += 1;
        }
        id
    }

    fn is_empty(self) -> bool {
        self.start > self.end
    }
}

#[derive(Default)]
pub(crate) struct PayloadAnchorRegistry {
    dense_states: DenseIdMap<PayloadAnchorSlot>,
    sparse_states: HashMap<usize, PayloadAnchorSlot>,
    free_ids: BinaryHeap<Reverse<FreePayloadAnchorIdRange>>,
    reused_generations: HashMap<u32, u32>,
    next_id: usize,
    active_count: usize,
    detached_count: usize,
    free_count: usize,
}

impl PayloadAnchorRegistry {
    const DENSE_STORAGE_ID_LIMIT: usize = 65_536;

    pub(super) fn new() -> Self {
        Self {
            dense_states: DenseIdMap::new(),
            sparse_states: HashMap::default(),
            free_ids: BinaryHeap::new(),
            reused_generations: HashMap::default(),
            next_id: 1,
            active_count: 0,
            detached_count: 0,
            free_count: 0,
        }
    }

    pub(super) fn allocate(&mut self) -> PayloadAnchor {
        let anchor = if let Some((id, generation)) = self.pop_free_id() {
            PayloadAnchor::new(id as usize, generation)
        } else {
            let anchor = PayloadAnchor::new(self.next_id, 1);
            self.next_id = self
                .next_id
                .checked_add(1)
                .expect("payload anchor counter overflow");
            anchor
        };
        let replaced = self.set_state(anchor, PayloadAnchorState::Detached);
        debug_assert!(
            replaced.is_none(),
            "payload anchors must not keep slots while reusable"
        );
        anchor
    }

    pub(super) fn set_active(&mut self, anchor: PayloadAnchor, owner: AnchorId, index: usize) {
        self.set_state(anchor, PayloadAnchorState::Active { owner, index });
        debug_assert_eq!(self.active_location(anchor), Some((owner, index)));
    }

    pub(super) fn mark_detached(&mut self, anchor: PayloadAnchor) {
        self.set_state(anchor, PayloadAnchorState::Detached);
    }

    pub(super) fn active_location(&self, anchor: PayloadAnchor) -> Option<(AnchorId, usize)> {
        let slot = self.slot(anchor)?;
        if slot.generation != anchor.generation() {
            return None;
        }
        match slot.state {
            PayloadAnchorState::Active { owner, index } => Some((owner, index)),
            PayloadAnchorState::Detached => None,
        }
    }

    pub(super) fn is_detached(&self, anchor: PayloadAnchor) -> bool {
        let Some(slot) = self.slot(anchor) else {
            return false;
        };
        slot.generation == anchor.generation() && matches!(slot.state, PayloadAnchorState::Detached)
    }

    pub(super) fn active_len(&self) -> usize {
        self.active_count
    }

    pub(super) fn slot_len(&self) -> usize {
        self.dense_states.len() + self.sparse_states.len()
    }

    pub(super) fn detached_len(&self) -> usize {
        self.detached_count
    }

    pub(super) fn invalidated_len(&self) -> usize {
        self.free_count
    }

    pub(super) fn free_len(&self) -> usize {
        self.free_count
    }

    pub(super) fn capacity(&self) -> usize {
        self.dense_states.capacity() + self.sparse_states.capacity()
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.dense_states.capacity() * mem::size_of::<Option<PayloadAnchorSlot>>()
            + self.sparse_states.capacity() * mem::size_of::<(usize, PayloadAnchorSlot)>()
            + self.free_ids.capacity() * mem::size_of::<Reverse<FreePayloadAnchorIdRange>>()
            + self.reused_generations.capacity() * mem::size_of::<(u32, u32)>()
    }

    pub(super) fn active_entries(
        &self,
    ) -> impl Iterator<Item = (PayloadAnchor, (AnchorId, usize))> + '_ {
        self.anchor_slots()
            .filter_map(|(id, slot)| match slot.state {
                PayloadAnchorState::Active { owner, index } => {
                    Some((PayloadAnchor::new(id, slot.generation), (owner, index)))
                }
                PayloadAnchorState::Detached => None,
            })
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn state_kind(&self, anchor: PayloadAnchor) -> Option<PayloadAnchorLifecycle> {
        if let Some(slot) = self.slot(anchor) {
            if slot.generation != anchor.generation() {
                return None;
            }
            return Some(match slot.state {
                PayloadAnchorState::Active { .. } => PayloadAnchorLifecycle::Active,
                PayloadAnchorState::Detached => PayloadAnchorLifecycle::Detached,
            });
        }
        let id = u32::try_from(anchor.id()).ok()?;
        let next_generation = anchor.generation().checked_add(1)?;
        (self.contains_free_id(id) && self.reused_generation(id) == next_generation)
            .then_some(PayloadAnchorLifecycle::Invalidated)
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn validate_integrity(&self) -> Result<(), SlotInvariantError> {
        let mut active_count = 0usize;
        let mut detached_count = 0usize;
        for (id, slot) in self.anchor_slots() {
            if id == 0 || id >= self.next_id {
                return Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                    detail: "registered payload anchor id must be allocated",
                    payload_anchor_id: Some(id),
                    expected: self.next_id,
                    actual: id,
                });
            }
            match slot.state {
                PayloadAnchorState::Active { .. } => active_count += 1,
                PayloadAnchorState::Detached => detached_count += 1,
            }
        }

        self.validate_state_count("active", self.active_count, active_count)?;
        self.validate_state_count("detached", self.detached_count, detached_count)?;

        let mut free_ranges = self
            .free_ids
            .iter()
            .map(|Reverse(range)| *range)
            .collect::<Vec<_>>();
        free_ranges.sort_unstable();

        let mut free_count = 0usize;
        let mut previous_range_end = None::<u32>;
        for range in &free_ranges {
            if range.is_empty() {
                return Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                    detail: "free payload anchor range must not be empty",
                    payload_anchor_id: Some(range.start as usize),
                    expected: 1,
                    actual: 0,
                });
            }
            if range.start == 0 || range.end as usize >= self.next_id {
                return Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                    detail: "free payload anchor id must be allocated",
                    payload_anchor_id: Some(range.start as usize),
                    expected: self.next_id,
                    actual: range.end as usize,
                });
            }
            if previous_range_end.is_some_and(|end| range.start <= end) {
                return Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                    detail: "free payload anchor id must be unique",
                    payload_anchor_id: Some(range.start as usize),
                    expected: 1,
                    actual: 2,
                });
            }
            previous_range_end = Some(range.end);
            free_count += range.len();
        }
        self.validate_state_count("free", self.free_count, free_count)?;

        for (id, _) in self.anchor_slots() {
            let id = u32::try_from(id).expect("payload anchor id must fit u32");
            if self.contains_free_id(id) {
                return Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                    detail: "free payload anchor id must not be active or detached",
                    payload_anchor_id: Some(id as usize),
                    expected: 0,
                    actual: 1,
                });
            }
        }

        for &id in self.reused_generations.keys() {
            if !self.contains_free_id(id) {
                return Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                    detail: "reused payload anchor generation must belong to a free id",
                    payload_anchor_id: Some(id as usize),
                    expected: 1,
                    actual: 0,
                });
            }
        }

        Ok(())
    }

    pub(super) fn bump_generation(&mut self, anchor: PayloadAnchor) -> Option<PayloadAnchor> {
        let slot = self.slot_mut(anchor)?;
        if slot.generation != anchor.generation() {
            return None;
        }
        let next_generation = anchor
            .generation()
            .checked_add(1)
            .expect("payload anchor generation counter overflow");
        slot.generation = next_generation;
        Some(anchor.with_generation(next_generation))
    }

    pub(super) fn invalidate(&mut self, anchor: PayloadAnchor) -> bool {
        let Some(slot) = self.slot(anchor) else {
            return false;
        };
        if slot.generation != anchor.generation() {
            return false;
        }
        let next_generation = anchor
            .generation()
            .checked_add(1)
            .expect("payload anchor generation counter overflow");
        let removed = self.remove_slot(anchor.id());
        let removed = removed.expect("validated payload anchor state must remove");
        self.adjust_state_counts(Some(removed.state), None);
        self.enqueue_reuse(anchor, next_generation);
        true
    }

    pub(super) fn clear(&mut self) {
        self.dense_states.clear();
        self.sparse_states.clear();
        self.free_ids.clear();
        self.reused_generations.clear();
        self.next_id = 1;
        self.active_count = 0;
        self.detached_count = 0;
        self.free_count = 0;
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.coalesce_free_id_ranges();
        self.dense_states.shrink_to_fit();
        self.sparse_states.shrink_to_fit();
        self.free_ids.shrink_to_fit();
        self.reused_generations.shrink_to_fit();
    }

    fn slot(&self, anchor: PayloadAnchor) -> Option<&PayloadAnchorSlot> {
        self.slot_by_id(anchor.id())
    }

    fn slot_mut(&mut self, anchor: PayloadAnchor) -> Option<&mut PayloadAnchorSlot> {
        if Self::uses_dense_storage(anchor.id()) {
            self.dense_states.get_mut(anchor.id())
        } else {
            self.sparse_states.get_mut(&anchor.id())
        }
    }

    fn slot_by_id(&self, id: usize) -> Option<&PayloadAnchorSlot> {
        if Self::uses_dense_storage(id) {
            self.dense_states.get(id)
        } else {
            self.sparse_states.get(&id)
        }
    }

    fn anchor_slots(&self) -> impl Iterator<Item = (usize, &PayloadAnchorSlot)> + '_ {
        self.dense_states
            .iter()
            .chain(self.sparse_states.iter().map(|(&id, slot)| (id, slot)))
    }

    fn insert_slot(
        &mut self,
        anchor: PayloadAnchor,
        state: PayloadAnchorState,
    ) -> Option<PayloadAnchorSlot> {
        let slot = PayloadAnchorSlot {
            generation: anchor.generation(),
            state,
        };
        if Self::uses_dense_storage(anchor.id()) {
            self.dense_states.insert(anchor.id(), slot)
        } else {
            self.sparse_states.insert(anchor.id(), slot)
        }
    }

    fn remove_slot(&mut self, id: usize) -> Option<PayloadAnchorSlot> {
        if Self::uses_dense_storage(id) {
            self.dense_states.remove(id)
        } else {
            self.sparse_states.remove(&id)
        }
    }

    fn uses_dense_storage(id: usize) -> bool {
        id <= Self::DENSE_STORAGE_ID_LIMIT
    }

    fn set_state(
        &mut self,
        anchor: PayloadAnchor,
        state: PayloadAnchorState,
    ) -> Option<PayloadAnchorState> {
        if let Some(slot) = self.slot(anchor) {
            if slot.generation != anchor.generation() {
                return None;
            }
        }
        let previous = self.insert_slot(anchor, state);
        self.adjust_state_counts(previous.map(|slot| slot.state), Some(state));
        previous.map(|slot| slot.state)
    }

    fn adjust_state_counts(
        &mut self,
        previous: Option<PayloadAnchorState>,
        next: Option<PayloadAnchorState>,
    ) {
        if matches!(previous, Some(PayloadAnchorState::Active { .. })) {
            self.active_count -= 1;
        }
        if matches!(previous, Some(PayloadAnchorState::Detached)) {
            self.detached_count -= 1;
        }
        if matches!(next, Some(PayloadAnchorState::Active { .. })) {
            self.active_count += 1;
        }
        if matches!(next, Some(PayloadAnchorState::Detached)) {
            self.detached_count += 1;
        }
    }

    fn enqueue_reuse(&mut self, anchor: PayloadAnchor, next_generation: u32) {
        let id = u32::try_from(anchor.id()).expect("payload anchor id must fit u32");
        self.free_ids
            .push(Reverse(FreePayloadAnchorIdRange::singleton(id)));
        self.free_count += 1;
        if next_generation == 2 {
            self.reused_generations.remove(&id);
        } else {
            self.reused_generations.insert(id, next_generation);
        }
    }

    fn pop_free_id(&mut self) -> Option<(u32, u32)> {
        let Reverse(mut range) = self.free_ids.pop()?;
        let id = range.pop_front();
        let generation = self.reused_generation(id);
        self.free_count -= 1;
        if !range.is_empty() {
            self.free_ids.push(Reverse(range));
        }
        self.reused_generations.remove(&id);
        Some((id, generation))
    }

    fn reused_generation(&self, id: u32) -> u32 {
        self.reused_generations.get(&id).copied().unwrap_or(2)
    }

    fn contains_free_id(&self, id: u32) -> bool {
        self.free_ids
            .iter()
            .any(|Reverse(range)| range.contains(id))
    }

    fn coalesce_free_id_ranges(&mut self) {
        if self.free_ids.len() <= 1 {
            return;
        }

        let mut ranges = self
            .free_ids
            .drain()
            .map(|Reverse(range)| range)
            .collect::<Vec<_>>();
        ranges.sort_unstable();

        let mut merged = Vec::<FreePayloadAnchorIdRange>::new();
        for range in ranges {
            if range.is_empty() {
                continue;
            }
            if let Some(last) = merged.last_mut() {
                if range.start <= last.end.saturating_add(1) {
                    last.end = last.end.max(range.end);
                    continue;
                }
            }
            merged.push(range);
        }

        self.free_count = merged.iter().map(|range| range.len()).sum();
        self.free_ids = merged.into_iter().map(Reverse).collect();
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
        Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
            detail,
            payload_anchor_id: None,
            expected,
            actual,
        })
    }
}

impl SlotTable {
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn payload_anchor_lifecycle(
        &self,
        anchor: PayloadAnchor,
    ) -> Option<PayloadAnchorLifecycle> {
        self.payload_anchors.state_kind(anchor)
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn payload_anchor_active_location(
        &self,
        anchor: PayloadAnchor,
    ) -> Option<(AnchorId, usize)> {
        self.payload_anchors.active_location(anchor)
    }

    pub(crate) fn invalidate_detached_subtree_payload_anchors(
        &mut self,
        subtree: &DetachedSubtree,
    ) {
        self.invalidate_payload_anchors(&subtree.payloads);
    }

    pub(super) fn mark_payload_anchors_detached(&mut self, payloads: &[PayloadRecord]) {
        for payload in payloads {
            self.payload_anchors.mark_detached(payload.anchor);
        }
    }

    pub(super) fn invalidate_payload_anchors(&mut self, payloads: &[PayloadRecord]) {
        let mut removed = false;
        for payload in payloads {
            removed |= self.payload_anchors.invalidate(payload.anchor);
        }
        if removed {
            self.payload_anchors.shrink_to_fit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_integrity_rejects_free_active_payload_anchor_id() {
        let mut registry = PayloadAnchorRegistry::new();
        let anchor = registry.allocate();
        let id = u32::try_from(anchor.id()).unwrap();
        registry
            .free_ids
            .push(Reverse(FreePayloadAnchorIdRange::singleton(id)));
        registry.free_count += 1;

        assert_eq!(
            registry.validate_integrity(),
            Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                detail: "free payload anchor id must not be active or detached",
                payload_anchor_id: Some(anchor.id()),
                expected: 0,
                actual: 1,
            })
        );
    }

    #[test]
    fn registry_integrity_rejects_reused_generation_without_free_id() {
        let mut registry = PayloadAnchorRegistry::new();
        registry.reused_generations.insert(7, 3);

        assert_eq!(
            registry.validate_integrity(),
            Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                detail: "reused payload anchor generation must belong to a free id",
                payload_anchor_id: Some(7),
                expected: 1,
                actual: 0,
            })
        );
    }

    #[test]
    fn invalidated_payload_anchor_ids_coalesce_into_compact_free_ranges() {
        let mut registry = PayloadAnchorRegistry::new();
        let anchors = (0..16).map(|_| registry.allocate()).collect::<Vec<_>>();
        for anchor in &anchors {
            assert!(registry.invalidate(*anchor));
        }

        registry.shrink_to_fit();

        assert_eq!(registry.slot_len(), 0);
        assert_eq!(registry.free_len(), anchors.len());
        assert_eq!(registry.free_ids.len(), 1);
        assert_eq!(registry.validate_integrity(), Ok(()));

        let reused = registry.allocate();
        assert_eq!(reused, anchors[0].with_generation(2));
        assert_eq!(registry.free_len(), anchors.len() - 1);
        assert_eq!(
            registry.state_kind(anchors[1]),
            Some(PayloadAnchorLifecycle::Invalidated)
        );
    }

    #[test]
    fn sparse_payload_anchor_ids_do_not_grow_dense_registry_storage() {
        let mut registry = PayloadAnchorRegistry::new();
        let anchor = PayloadAnchor::new(2_500_000, 1);
        registry.next_id = anchor.id() + 1;

        registry.set_active(anchor, AnchorId::new(1), 0);

        assert_eq!(
            registry.active_location(anchor),
            Some((AnchorId::new(1), 0))
        );
        assert_eq!(registry.slot_len(), 1);
        assert!(
            registry.capacity() < 128,
            "sparse payload ids must not allocate dense registry storage: capacity={}",
            registry.capacity()
        );
        assert_eq!(registry.validate_integrity(), Ok(()));
    }
}

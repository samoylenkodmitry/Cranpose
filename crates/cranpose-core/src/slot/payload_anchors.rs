use std::{cmp::Reverse, collections::BinaryHeap, mem};

#[cfg(any(test, debug_assertions))]
use super::SlotInvariantError;
use super::{
    generational_registry::{GenerationalRegistryStorage, RegistryState},
    DetachedSubtree, PayloadAnchor, PayloadRecord, SlotTable,
};
use crate::{collections::map::HashMap, AnchorId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PayloadAnchorState {
    Active { owner: AnchorId, index: usize },
    Detached,
}

impl RegistryState for PayloadAnchorState {
    fn is_active(self) -> bool {
        matches!(self, Self::Active { .. })
    }

    fn is_detached(self) -> bool {
        matches!(self, Self::Detached)
    }
}

#[cfg(any(test, debug_assertions))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PayloadAnchorLifecycle {
    Active,
    Detached,
    Invalidated,
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

    #[cfg(any(test, debug_assertions))]
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
    storage: GenerationalRegistryStorage<PayloadAnchorState>,
    free_ids: BinaryHeap<Reverse<FreePayloadAnchorIdRange>>,
    reused_generations: HashMap<u32, u32>,
    next_id: usize,
    free_count: usize,
}

impl PayloadAnchorRegistry {
    pub(super) fn new() -> Self {
        Self {
            storage: GenerationalRegistryStorage::new(),
            free_ids: BinaryHeap::new(),
            reused_generations: HashMap::default(),
            next_id: 1,
            free_count: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn allocate(&mut self) -> PayloadAnchor {
        self.try_allocate().unwrap_or(PayloadAnchor::INVALID)
    }

    pub(super) fn try_allocate(&mut self) -> Option<PayloadAnchor> {
        let anchor = if let Some((id, generation)) = self.pop_free_id() {
            PayloadAnchor::new(id as usize, generation)
        } else {
            if self.next_id > u32::MAX as usize {
                log::error!(
                    "slot table rejected payload anchor allocation because id counter {} exceeds u32 storage",
                    self.next_id
                );
                return None;
            }
            let anchor = PayloadAnchor::new(self.next_id, 1);
            self.next_id = self.next_id.saturating_add(1);
            anchor
        };
        let replaced = self.set_state(anchor, PayloadAnchorState::Detached);
        debug_assert!(
            replaced.is_none(),
            "payload anchors must not keep slots while reusable"
        );
        Some(anchor)
    }

    #[cfg(test)]
    pub(super) fn set_next_id_for_test(&mut self, next_id: usize) {
        self.next_id = next_id;
    }

    pub(super) fn set_active(&mut self, anchor: PayloadAnchor, owner: AnchorId, index: usize) {
        let previous = self.set_state(anchor, PayloadAnchorState::Active { owner, index });
        if previous.is_none() {
            log::error!(
                "slot table ignored active-state update for stale payload anchor {anchor:?}"
            );
            return;
        }
        debug_assert_eq!(self.active_location(anchor), Some((owner, index)));
    }

    pub(super) fn mark_detached(&mut self, anchor: PayloadAnchor) {
        let previous = self.set_state(anchor, PayloadAnchorState::Detached);
        if previous.is_none() {
            log::error!("slot table ignored detach for stale payload anchor {anchor:?}");
        }
    }

    pub(super) fn active_location(&self, anchor: PayloadAnchor) -> Option<(AnchorId, usize)> {
        let slot = self.storage.slot(anchor.id())?;
        if slot.generation != anchor.generation() {
            return None;
        }
        match slot.state {
            PayloadAnchorState::Active { owner, index } => Some((owner, index)),
            PayloadAnchorState::Detached => None,
        }
    }

    pub(super) fn is_detached(&self, anchor: PayloadAnchor) -> bool {
        let Some(slot) = self.storage.slot(anchor.id()) else {
            return false;
        };
        slot.generation == anchor.generation() && matches!(slot.state, PayloadAnchorState::Detached)
    }

    pub(super) fn active_len(&self) -> usize {
        self.storage.active_len()
    }

    pub(super) fn slot_len(&self) -> usize {
        self.storage.slot_len()
    }

    pub(super) fn detached_len(&self) -> usize {
        self.storage.detached_len()
    }

    pub(super) fn invalidated_len(&self) -> usize {
        self.storage.invalidated_len()
    }

    pub(super) fn free_len(&self) -> usize {
        self.free_count
    }

    pub(super) fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.storage.heap_bytes()
            + self.free_ids.capacity() * mem::size_of::<Reverse<FreePayloadAnchorIdRange>>()
            + self.reused_generations.capacity() * mem::size_of::<(u32, u32)>()
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn active_entries(
        &self,
    ) -> impl Iterator<Item = (PayloadAnchor, (AnchorId, usize))> + '_ {
        self.storage
            .slots()
            .filter_map(|(id, slot)| match slot.state {
                PayloadAnchorState::Active { owner, index } => {
                    Some((PayloadAnchor::new(id, slot.generation), (owner, index)))
                }
                PayloadAnchorState::Detached => None,
            })
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn state_kind(&self, anchor: PayloadAnchor) -> Option<PayloadAnchorLifecycle> {
        if let Some(slot) = self.storage.slot(anchor.id()) {
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
        for (id, slot) in self.storage.slots() {
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

        self.validate_state_count("active", self.storage.active_len(), active_count)?;
        self.validate_state_count("detached", self.storage.detached_len(), detached_count)?;

        let free_ranges = self.sorted_free_ranges();

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

        for (id, _) in self.storage.slots() {
            let id = u32::try_from(id).expect("payload anchor id must fit u32");
            if Self::sorted_ranges_contain(&free_ranges, id) {
                return Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
                    detail: "free payload anchor id must not be active or detached",
                    payload_anchor_id: Some(id as usize),
                    expected: 0,
                    actual: 1,
                });
            }
        }

        for &id in self.reused_generations.keys() {
            if !Self::sorted_ranges_contain(&free_ranges, id) {
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
        let slot = self.storage.slot_mut(anchor.id())?;
        if slot.generation != anchor.generation() {
            return None;
        }
        let Some(next_generation) = anchor.generation().checked_add(1) else {
            log::error!(
                "slot table cannot bump payload anchor {anchor:?}: generation counter overflow"
            );
            return None;
        };
        slot.generation = next_generation;
        Some(anchor.with_generation(next_generation))
    }

    pub(super) fn invalidate(&mut self, anchor: PayloadAnchor) -> bool {
        let Some(slot) = self.storage.slot(anchor.id()) else {
            return false;
        };
        if slot.generation != anchor.generation() {
            return false;
        }
        let Some(next_generation) = anchor.generation().checked_add(1) else {
            log::error!(
                "slot table cannot invalidate payload anchor {anchor:?}: generation counter overflow"
            );
            return false;
        };
        if self.storage.remove_state(anchor.id()).is_none() {
            log::error!(
                "slot table ignored invalidation for payload anchor removed during validation {anchor:?}"
            );
            return false;
        }
        self.enqueue_reuse(anchor, next_generation);
        true
    }

    pub(super) fn clear(&mut self) {
        self.storage.clear();
        self.free_ids.clear();
        self.reused_generations.clear();
        self.next_id = 1;
        self.free_count = 0;
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.coalesce_free_id_ranges();
        self.storage.shrink_to_fit();
        self.free_ids.shrink_to_fit();
        self.reused_generations.shrink_to_fit();
    }

    fn maybe_shrink_sparse_storage(&mut self) {
        if self.storage.maybe_shrink_sparse_storage() {
            self.coalesce_free_id_ranges();
            self.free_ids.shrink_to_fit();
            self.reused_generations.shrink_to_fit();
        }
    }

    fn set_state(
        &mut self,
        anchor: PayloadAnchor,
        state: PayloadAnchorState,
    ) -> Option<PayloadAnchorState> {
        self.storage
            .set_state(anchor.id(), anchor.generation(), state)
    }

    fn enqueue_reuse(&mut self, anchor: PayloadAnchor, next_generation: u32) {
        let Ok(id) = u32::try_from(anchor.id()) else {
            log::error!("slot table cannot reuse payload anchor {anchor:?}: id exceeds u32");
            return;
        };
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

    #[cfg(any(test, debug_assertions))]
    fn contains_free_id(&self, id: u32) -> bool {
        self.free_ids
            .iter()
            .any(|Reverse(range)| range.contains(id))
    }

    #[cfg(any(test, debug_assertions))]
    fn sorted_free_ranges(&self) -> Vec<FreePayloadAnchorIdRange> {
        let mut ranges = self
            .free_ids
            .iter()
            .map(|Reverse(range)| *range)
            .collect::<Vec<_>>();
        ranges.sort_unstable();
        ranges
    }

    #[cfg(any(test, debug_assertions))]
    fn sorted_ranges_contain(ranges: &[FreePayloadAnchorIdRange], id: u32) -> bool {
        ranges
            .binary_search_by(|range| {
                if id < range.start {
                    std::cmp::Ordering::Greater
                } else if id > range.end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
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
            self.payload_anchors.maybe_shrink_sparse_storage();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_anchor_registry_lifecycle_reuses_invalidated_id_with_generation_bump() {
        let mut registry = PayloadAnchorRegistry::new();
        let owner = AnchorId::new(10);
        let stale_anchor = registry.allocate();

        assert_eq!(
            registry.state_kind(stale_anchor),
            Some(PayloadAnchorLifecycle::Detached)
        );
        assert_eq!(registry.active_len(), 0);
        assert_eq!(registry.detached_len(), 1);
        assert_eq!(registry.invalidated_len(), 0);
        assert_eq!(registry.free_len(), 0);

        registry.set_active(stale_anchor, owner, 3);
        assert_eq!(registry.active_location(stale_anchor), Some((owner, 3)));
        assert_eq!(
            registry.state_kind(stale_anchor),
            Some(PayloadAnchorLifecycle::Active)
        );
        assert_eq!(registry.active_len(), 1);
        assert_eq!(registry.detached_len(), 0);

        registry.mark_detached(stale_anchor);
        assert!(registry.is_detached(stale_anchor));
        assert_eq!(registry.active_location(stale_anchor), None);
        assert_eq!(registry.active_len(), 0);
        assert_eq!(registry.detached_len(), 1);

        assert!(registry.invalidate(stale_anchor));
        assert_eq!(
            registry.state_kind(stale_anchor),
            Some(PayloadAnchorLifecycle::Invalidated)
        );
        assert_eq!(registry.active_location(stale_anchor), None);
        assert_eq!(registry.active_len(), 0);
        assert_eq!(registry.detached_len(), 0);
        assert_eq!(registry.invalidated_len(), 0);
        assert_eq!(registry.free_len(), 1);
        assert_eq!(registry.validate_integrity(), Ok(()));

        let reused_anchor = registry.allocate();
        assert_eq!(reused_anchor.id(), stale_anchor.id());
        assert_eq!(reused_anchor.generation(), stale_anchor.generation() + 1);
        assert_eq!(registry.state_kind(stale_anchor), None);
        assert_eq!(
            registry.state_kind(reused_anchor),
            Some(PayloadAnchorLifecycle::Detached)
        );
        assert_eq!(registry.invalidated_len(), 0);
        assert_eq!(registry.free_len(), 0);

        registry.set_active(stale_anchor, owner, 99);
        assert_eq!(
            registry.state_kind(reused_anchor),
            Some(PayloadAnchorLifecycle::Detached),
            "stale generation rejection must preserve the reused payload anchor state"
        );
        registry.mark_detached(stale_anchor);
        assert_eq!(
            registry.state_kind(reused_anchor),
            Some(PayloadAnchorLifecycle::Detached),
            "stale generation must not detach a reused payload anchor id"
        );

        registry.set_active(reused_anchor, owner, 5);
        assert_eq!(registry.active_location(reused_anchor), Some((owner, 5)));
        assert_eq!(registry.active_location(stale_anchor), None);
        assert_eq!(registry.active_len(), 1);
        assert_eq!(registry.detached_len(), 0);
        assert_eq!(registry.invalidated_len(), 0);
        assert_eq!(registry.free_len(), 0);
        assert_eq!(registry.validate_integrity(), Ok(()));
    }

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
        assert_eq!(registry.invalidated_len(), 0);
        assert_eq!(registry.free_len(), anchors.len());
        assert_eq!(registry.free_ids.len(), 1);
        assert_eq!(registry.validate_integrity(), Ok(()));

        let reused = registry.allocate();
        assert_eq!(reused, anchors[0].with_generation(2));
        assert_eq!(registry.invalidated_len(), 0);
        assert_eq!(registry.free_len(), anchors.len() - 1);
        assert_eq!(
            registry.state_kind(anchors[1]),
            Some(PayloadAnchorLifecycle::Invalidated)
        );
    }

    #[test]
    fn payload_anchor_disposal_keeps_dense_storage_for_hot_path_reuse() {
        let mut table = SlotTable::new();
        let owner = table.anchors.allocate();
        table.anchors.set_active(owner, 0);

        let mut payloads = Vec::new();
        for value in 0..32_i32 {
            let anchor = table.payload_anchors.allocate();
            table
                .payload_anchors
                .set_active(anchor, owner, value as usize);
            payloads.push(PayloadRecord {
                owner,
                anchor,
                type_id: std::any::TypeId::of::<i32>(),
                type_name: std::any::type_name::<i32>(),
                kind: crate::slot::PayloadKind::Internal,
                value: Box::new(value),
                fresh: None,
            });
        }

        let dense_capacity_before = table.payload_anchors.storage.dense_capacity();
        assert!(dense_capacity_before >= payloads.len());

        table.invalidate_payload_anchors(&payloads);

        assert_eq!(table.payload_anchors.active_len(), 0);
        assert_eq!(table.payload_anchors.invalidated_len(), 0);
        assert_eq!(table.payload_anchors.free_len(), payloads.len());
        assert_eq!(
            table.payload_anchors.storage.dense_capacity(),
            dense_capacity_before,
            "payload disposal must not compact dense anchor storage on the mutation hot path"
        );
        assert_eq!(table.payload_anchors.validate_integrity(), Ok(()));
    }

    #[test]
    fn sparse_payload_anchor_ids_do_not_grow_dense_registry_storage() {
        let mut registry = PayloadAnchorRegistry::new();
        let anchor = PayloadAnchor::new(2_500_000, 1);
        registry.next_id = anchor.id() + 1;

        assert!(registry
            .set_state(anchor, PayloadAnchorState::Detached)
            .is_none());
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

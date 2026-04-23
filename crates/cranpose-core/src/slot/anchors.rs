use super::{dense_id_map::DenseIdMap, DetachedSubtree, GroupRecord, SlotTable};
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
    states: DenseIdMap<AnchorSlot>,
    free_ids: BinaryHeap<Reverse<u32>>,
    reused_generations: HashMap<u32, u32>,
    next_anchor: usize,
    active_count: usize,
    detached_count: usize,
}

impl AnchorRegistry {
    pub(super) fn new() -> Self {
        Self {
            states: DenseIdMap::new(),
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
            self.next_anchor += 1;
            anchor
        };
        let replaced = self.set_state(anchor, AnchorState::Invalidated);
        debug_assert!(replaced.is_none(), "group anchors must stay unique");
        anchor
    }

    pub(super) fn state(&self, anchor: AnchorId) -> Option<AnchorState> {
        let index = Self::anchor_index(anchor)?;
        let slot = self.states.get(index)?;
        (slot.generation == anchor.generation).then_some(slot.state)
    }

    pub(super) fn active_index(&self, anchor: AnchorId) -> Option<usize> {
        match self.state(anchor) {
            Some(AnchorState::Active(index)) => Some(index),
            _ => None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn contains_active(&self, anchor: AnchorId) -> bool {
        self.active_index(anchor).is_some()
    }

    pub(super) fn active_len(&self) -> usize {
        self.active_count
    }

    pub(super) fn slot_len(&self) -> usize {
        self.states.len()
    }

    pub(super) fn sparse_slot_len(&self) -> usize {
        let reserved_zero_slot = usize::from(self.states.storage_len() > 0);
        self.states
            .storage_len()
            .saturating_sub(self.states.len() + reserved_zero_slot)
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
        self.states
            .iter()
            .filter_map(|(index, slot)| match slot.state {
                AnchorState::Active(group_index) => Some((
                    AnchorId {
                        id: index as u32,
                        generation: slot.generation,
                    },
                    group_index,
                )),
                AnchorState::Detached | AnchorState::Invalidated => None,
            })
    }

    pub(super) fn capacity(&self) -> usize {
        self.states.capacity()
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.states.capacity() * mem::size_of::<Option<AnchorSlot>>()
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn invalidate(&mut self, anchor: AnchorId) {
        if self.invalidate_state(anchor) {
            self.maybe_shrink_sparse_storage();
        }
    }

    pub(super) fn mark_detached_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            self.mark_detached(group.anchor);
        }
    }

    pub(super) fn clear(&mut self) {
        self.states.clear();
        self.free_ids.clear();
        self.reused_generations.clear();
        self.next_anchor = 1;
        self.active_count = 0;
        self.detached_count = 0;
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.states.shrink_to_fit();
        self.free_ids.shrink_to_fit();
        self.reused_generations.shrink_to_fit();
    }

    fn invalidate_state(&mut self, anchor: AnchorId) -> bool {
        let Some(index) = Self::anchor_index(anchor) else {
            return false;
        };
        let Some(slot) = self.states.get(index) else {
            return false;
        };
        if slot.generation != anchor.generation {
            return false;
        }
        let removed = self
            .states
            .remove(index)
            .expect("validated anchor state must remove");
        self.adjust_state_counts(Some(removed.state), None);
        self.enqueue_reuse(anchor);
        true
    }

    fn set_state(&mut self, anchor: AnchorId, state: AnchorState) -> Option<AnchorState> {
        let index = Self::anchor_index(anchor)?;
        if let Some(slot) = self.states.get(index) {
            if slot.generation != anchor.generation {
                return None;
            }
        }
        let previous = self.states.insert(
            index,
            AnchorSlot {
                generation: anchor.generation,
                state,
            },
        );
        self.adjust_state_counts(previous.map(|slot| slot.state), Some(state));
        previous.map(|slot| slot.state)
    }

    fn anchor_index(anchor: AnchorId) -> Option<usize> {
        anchor.is_valid().then_some(anchor.id as usize)
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
        if self.states.capacity() <= 4096 {
            return;
        }
        if self.states.len().saturating_mul(4) >= self.states.capacity() {
            return;
        }
        self.states.shrink_to_fit();
        self.free_ids.shrink_to_fit();
        self.reused_generations.shrink_to_fit();
    }
}

impl SlotTable {
    pub(crate) fn anchor_state(&self, anchor: AnchorId) -> Option<AnchorState> {
        self.anchors.state(anchor)
    }

    pub(crate) fn invalidate_detached_subtree_anchors(&mut self, subtree: &DetachedSubtree) {
        let mut removed = false;
        for anchor in subtree.group_anchors() {
            removed |= self.anchors.invalidate_state(anchor);
        }
        if removed {
            self.anchors.maybe_shrink_sparse_storage();
        }
    }

    pub(crate) fn compact_anchor_namespace(
        &mut self,
        retention: Option<&mut RetentionManager>,
        mut scope_for_id: impl FnMut(ScopeId) -> Option<RecomposeScope>,
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
        let sparse_namespace = max_anchor_id > total_group_count.max(256) * 4;
        let sparse_capacity = self.anchors.capacity() > total_group_count.max(256) * 8;
        if !sparse_namespace && !sparse_capacity {
            return;
        }

        let mut remapped = HashMap::default();
        let mut next_id = 1u32;
        for group in &self.groups {
            remapped.insert(
                group.anchor,
                AnchorId {
                    id: next_id,
                    generation: group.anchor.generation,
                },
            );
            next_id += 1;
        }
        if let Some(retention) = retention.as_ref() {
            for subtree in retention.subtrees() {
                for anchor in subtree.group_anchors() {
                    remapped.insert(
                        anchor,
                        AnchorId {
                            id: next_id,
                            generation: anchor.generation,
                        },
                    );
                    next_id += 1;
                }
            }
        }

        self.remap_active_group_anchors(&remapped, &mut scope_for_id);
        let detached_anchors = if let Some(retention) = retention {
            for subtree in retention.subtrees_mut() {
                self.remap_detached_group_anchors(subtree, &remapped, &mut scope_for_id);
            }
            retention
                .subtrees()
                .flat_map(DetachedSubtree::group_anchors)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        self.payload_locations.clear();
        self.rebuild_payload_locations_for_group_range(0, self.groups.len());
        self.recompute_scope_index();
        self.rebuild_anchor_registry(detached_anchors);
    }

    fn remap_active_group_anchors(
        &mut self,
        remapped: &HashMap<AnchorId, AnchorId>,
        scope_for_id: &mut impl FnMut(ScopeId) -> Option<RecomposeScope>,
    ) {
        for group in &mut self.groups {
            let old_anchor = group.anchor;
            let new_anchor = remapped
                .get(&old_anchor)
                .copied()
                .expect("active group anchor must remap");
            group.anchor = new_anchor;
            if group.parent_anchor.is_valid() {
                group.parent_anchor = remapped
                    .get(&group.parent_anchor)
                    .copied()
                    .expect("active parent anchor must remap");
            }
            if let Some(scope_id) = group.scope_id {
                if let Some(scope) = scope_for_id(scope_id) {
                    scope.set_group_anchor(new_anchor);
                }
            }
        }
        for payload in &mut self.payloads {
            payload.owner = remapped
                .get(&payload.owner)
                .copied()
                .expect("active payload owner must remap");
        }
        for node in &mut self.nodes {
            node.owner = remapped
                .get(&node.owner)
                .copied()
                .expect("active node owner must remap");
        }
    }

    fn remap_detached_group_anchors(
        &self,
        subtree: &mut DetachedSubtree,
        remapped: &HashMap<AnchorId, AnchorId>,
        scope_for_id: &mut impl FnMut(ScopeId) -> Option<RecomposeScope>,
    ) {
        for group in &mut subtree.groups {
            let old_anchor = group.anchor;
            let new_anchor = remapped
                .get(&old_anchor)
                .copied()
                .expect("detached group anchor must remap");
            group.anchor = new_anchor;
            if group.parent_anchor.is_valid() {
                group.parent_anchor = remapped
                    .get(&group.parent_anchor)
                    .copied()
                    .expect("detached parent anchor must remap");
            }
            if let Some(scope_id) = group.scope_id {
                if let Some(scope) = scope_for_id(scope_id) {
                    scope.set_group_anchor(new_anchor);
                }
            }
        }
        for payload in &mut subtree.payloads {
            payload.owner = remapped
                .get(&payload.owner)
                .copied()
                .expect("detached payload owner must remap");
        }
        for node in &mut subtree.nodes {
            node.owner = remapped
                .get(&node.owner)
                .copied()
                .expect("detached node owner must remap");
        }
        for anchor in &mut subtree.anchors.group_anchors {
            *anchor = remapped
                .get(anchor)
                .copied()
                .expect("detached anchor metadata must remap");
        }
    }

    fn rebuild_anchor_registry(&mut self, detached_anchors: Vec<AnchorId>) {
        self.anchors.clear();
        self.anchors.shrink_to_fit();
        for (index, group) in self.groups.iter().enumerate() {
            self.anchors.set_active(group.anchor, index);
        }
        let mut max_anchor_id = self
            .groups
            .iter()
            .map(|group| group.anchor.id as usize)
            .max()
            .unwrap_or(0);
        for anchor in detached_anchors {
            max_anchor_id = max_anchor_id.max(anchor.id as usize);
            self.anchors.mark_detached(anchor);
        }
        self.anchors.next_anchor = max_anchor_id + 1;
    }
}

use super::{dense_id_map::DenseIdMap, DetachedSubtree, PayloadRecord, SlotTable};
use crate::{slot_storage::PayloadAnchor, AnchorId};
use std::{cmp::Reverse, collections::BinaryHeap};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PayloadAnchorState {
    Active { owner: AnchorId, index: usize },
    Detached,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PayloadAnchorSlot {
    generation: u32,
    state: PayloadAnchorState,
}

#[derive(Default)]
pub(crate) struct PayloadAnchorRegistry {
    states: DenseIdMap<PayloadAnchorSlot>,
    free_ids: BinaryHeap<Reverse<u32>>,
    next_id: usize,
    active_count: usize,
    detached_count: usize,
    invalidated_count: usize,
}

impl PayloadAnchorRegistry {
    pub(super) fn new() -> Self {
        Self {
            states: DenseIdMap::new(),
            free_ids: BinaryHeap::new(),
            next_id: 1,
            active_count: 0,
            detached_count: 0,
            invalidated_count: 0,
        }
    }

    pub(super) fn allocate(&mut self) -> PayloadAnchor {
        let anchor = if let Some(Reverse(id)) = self.free_ids.pop() {
            let slot = self
                .states
                .get(id as usize)
                .expect("free payload anchor id must have invalidated state");
            debug_assert_eq!(slot.state, PayloadAnchorState::Invalidated);
            PayloadAnchor::new(id as usize, slot.generation)
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
            matches!(replaced, None | Some(PayloadAnchorState::Invalidated)),
            "payload anchors must be new or invalidated before allocation"
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
            PayloadAnchorState::Detached | PayloadAnchorState::Invalidated => None,
        }
    }

    pub(super) fn active_len(&self) -> usize {
        self.active_count
    }

    pub(super) fn active_entries(
        &self,
    ) -> impl Iterator<Item = (PayloadAnchor, (AnchorId, usize))> + '_ {
        self.states
            .iter()
            .filter_map(|(id, slot)| match slot.state {
                PayloadAnchorState::Active { owner, index } => {
                    Some((PayloadAnchor::new(id, slot.generation), (owner, index)))
                }
                PayloadAnchorState::Detached | PayloadAnchorState::Invalidated => None,
            })
    }

    pub(super) fn bump_generation(&mut self, anchor: PayloadAnchor) -> Option<PayloadAnchor> {
        let slot = self.states.get_mut(anchor.id())?;
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
        let slot = self
            .states
            .get_mut(anchor.id())
            .expect("validated payload anchor state must exist");
        let previous_state = slot.state;
        slot.generation = next_generation;
        slot.state = PayloadAnchorState::Invalidated;
        self.adjust_state_counts(Some(previous_state), Some(PayloadAnchorState::Invalidated));
        self.enqueue_reuse(anchor);
        true
    }

    pub(super) fn clear(&mut self) {
        self.states.clear();
        self.free_ids.clear();
        self.next_id = 1;
        self.active_count = 0;
        self.detached_count = 0;
        self.invalidated_count = 0;
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.states.shrink_to_fit();
        self.free_ids.shrink_to_fit();
    }

    fn slot(&self, anchor: PayloadAnchor) -> Option<&PayloadAnchorSlot> {
        self.states.get(anchor.id())
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
        let previous = self.states.insert(
            anchor.id(),
            PayloadAnchorSlot {
                generation: anchor.generation(),
                state,
            },
        );
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
        if matches!(previous, Some(PayloadAnchorState::Invalidated)) {
            self.invalidated_count -= 1;
        }
        if matches!(next, Some(PayloadAnchorState::Active { .. })) {
            self.active_count += 1;
        }
        if matches!(next, Some(PayloadAnchorState::Detached)) {
            self.detached_count += 1;
        }
        if matches!(next, Some(PayloadAnchorState::Invalidated)) {
            self.invalidated_count += 1;
        }
    }

    fn enqueue_reuse(&mut self, anchor: PayloadAnchor) {
        let id = u32::try_from(anchor.id()).expect("payload anchor id must fit u32");
        self.free_ids.push(Reverse(id));
    }
}

impl SlotTable {
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

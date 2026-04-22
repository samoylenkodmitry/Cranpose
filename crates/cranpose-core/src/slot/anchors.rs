use super::{DetachedSubtree, GroupRecord, SlotTable};
use crate::{collections::map::HashMap, AnchorId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnchorState {
    Active(usize),
    Detached,
    Invalidated,
}

#[derive(Default)]
pub(crate) struct AnchorRegistry {
    states: HashMap<AnchorId, AnchorState>,
    next_anchor: usize,
}

impl AnchorRegistry {
    pub(super) fn new() -> Self {
        Self {
            states: HashMap::default(),
            next_anchor: 1,
        }
    }

    pub(super) fn allocate(&mut self) -> AnchorId {
        let anchor = AnchorId::new(self.next_anchor);
        self.next_anchor += 1;
        let replaced = self.states.insert(anchor, AnchorState::Invalidated);
        debug_assert!(replaced.is_none(), "group anchors must stay unique");
        anchor
    }

    pub(super) fn state(&self, anchor: AnchorId) -> Option<AnchorState> {
        self.states.get(&anchor).copied()
    }

    pub(super) fn active_index(&self, anchor: AnchorId) -> Option<usize> {
        match self.state(anchor) {
            Some(AnchorState::Active(index)) => Some(index),
            _ => None,
        }
    }

    pub(super) fn contains_active(&self, anchor: AnchorId) -> bool {
        self.active_index(anchor).is_some()
    }

    pub(super) fn active_len(&self) -> usize {
        self.states
            .values()
            .filter(|state| matches!(state, AnchorState::Active(_)))
            .count()
    }

    pub(super) fn active_entries(&self) -> impl Iterator<Item = (AnchorId, usize)> + '_ {
        self.states
            .iter()
            .filter_map(|(&anchor, &state)| match state {
                AnchorState::Active(group_index) => Some((anchor, group_index)),
                AnchorState::Detached | AnchorState::Invalidated => None,
            })
    }

    pub(super) fn capacity(&self) -> usize {
        self.states.capacity()
    }

    pub(super) fn set_active(&mut self, anchor: AnchorId, group_index: usize) {
        self.states.insert(anchor, AnchorState::Active(group_index));
    }

    pub(super) fn mark_detached(&mut self, anchor: AnchorId) {
        if anchor.is_valid() {
            self.states.insert(anchor, AnchorState::Detached);
        }
    }

    pub(super) fn invalidate(&mut self, anchor: AnchorId) {
        if anchor.is_valid() {
            self.states.insert(anchor, AnchorState::Invalidated);
        }
    }

    pub(super) fn recompute_active(&mut self, groups: &[GroupRecord]) {
        for (group_index, group) in groups.iter().enumerate() {
            self.set_active(group.anchor, group_index);
        }
    }

    pub(super) fn mark_detached_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            self.mark_detached(group.anchor);
        }
    }

    pub(super) fn invalidate_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            self.invalidate(group.anchor);
        }
    }

    pub(super) fn clear(&mut self) {
        self.states.clear();
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.states
            .retain(|_, state| !matches!(state, AnchorState::Invalidated));
        self.states.shrink_to_fit();
    }
}

impl SlotTable {
    pub(crate) fn anchor_state(&self, anchor: AnchorId) -> Option<AnchorState> {
        self.anchors.state(anchor)
    }

    pub(crate) fn invalidate_detached_subtree_anchors(&mut self, subtree: &DetachedSubtree) {
        self.anchors.invalidate_groups(&subtree.groups);
    }
}

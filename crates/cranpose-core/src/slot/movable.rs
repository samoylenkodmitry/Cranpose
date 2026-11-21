//! Movable content: groups whose identity is a key alone, not a call site.
//!
//! A movable group can be emitted from any parent in a slot table and keep
//! its payloads, scopes and nodes. The table indexes every attached movable
//! by its key so a composing site can tell whether the content is still
//! attached elsewhere, and a detached subtree can give up the movables
//! nested inside it so each one is retained under its own identity.

use smallvec::SmallVec;

use super::{
    DetachedSubtree, GroupKey, GroupRecord, SlotTable, SlotWriteSession,
    segments::{NodeSegment, PayloadSegment, extract_subtree_segment},
};
use crate::{
    AnchorId, Key,
    collections::map::HashMap,
    movable_content::{disarm_remembered_movable, sole_owned_movable_id},
    slot::PayloadAnchor,
};

pub(crate) const MOVABLE_STATIC_KEY: Key = Key(0x6d6f_7661_626c_6521);

pub(crate) const MOVABLE_PLACEHOLDER_STATIC_KEY: Key = Key(0x6d6f_7661_626c_6520);

#[derive(Default)]
pub(crate) struct MovableIndex {
    anchors: HashMap<Key, AnchorId>,
}

impl MovableIndex {
    pub(in crate::slot) fn note_group(&mut self, key: GroupKey, anchor: AnchorId) {
        if let Some(id) = key.movable_id() {
            self.anchors.insert(id, anchor);
        }
    }

    pub(in crate::slot) fn note_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            self.note_group(group.key, group.anchor);
        }
    }

    pub(in crate::slot) fn forget_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            if let Some(id) = group.key.movable_id()
                && self.anchors.get(&id) == Some(&group.anchor)
            {
                self.anchors.remove(&id);
            }
        }
    }

    pub(in crate::slot) fn anchor(&self, id: Key) -> Option<AnchorId> {
        self.anchors.get(&id).copied()
    }

    pub(in crate::slot) fn clear(&mut self) {
        self.anchors.clear();
    }

    pub(in crate::slot) fn shrink_to_fit(&mut self) {
        self.anchors.shrink_to_fit();
    }
}

impl SlotTable {
    pub(crate) fn group_is_active(&self, anchor: AnchorId) -> bool {
        self.active_group_index(anchor).is_some()
    }

    fn movable_parent_anchor(&self, id: Key) -> Option<AnchorId> {
        let anchor = self.movables.anchor(id)?;
        let index = self.active_group_index(anchor)?;
        Some(self.groups[index].parent_anchor)
    }

    /// Whether this table currently holds the movable's content.
    pub(crate) fn movable_is_attached(&self, id: Key) -> bool {
        // The anchor's state answers without the group, which an insert run
        // of this table may hold aside while another table composes.
        self.movables
            .anchor(id)
            .is_some_and(|anchor| self.anchors.active_index(anchor).is_some())
    }

    /// Whether this table issued the anchors a detached subtree carries.
    ///
    /// A subtree detached from another slot table — the one a
    /// `SubcomposeLayout` keeps for its slot, say — names anchors that mean
    /// nothing here, and could collide with live ones.
    pub(crate) fn holds_anchors_of(&self, subtree: &DetachedSubtree) -> bool {
        subtree
            .group_anchors()
            .all(|anchor| self.anchors.is_detached(anchor))
            && subtree
                .payload_anchors()
                .all(|anchor| self.payload_anchors.is_detached(anchor))
    }

    /// Issues this table's own anchors for a subtree detached from another
    /// table, so movable content can arrive in a slot table that never held
    /// it. The subtree keeps its groups, payloads, nodes and scopes; only the
    /// names this table knows them by are new.
    pub(crate) fn adopt_detached_subtree(&mut self, subtree: &mut DetachedSubtree) -> bool {
        let mut groups: HashMap<AnchorId, AnchorId> = HashMap::default();
        for group in &subtree.groups {
            let Some(anchor) = self.anchors.try_allocate() else {
                return false;
            };
            self.anchors.mark_detached(anchor);
            groups.insert(group.anchor, anchor);
        }
        let mut payloads: HashMap<PayloadAnchor, PayloadAnchor> = HashMap::default();
        for payload in &subtree.payloads {
            let Some(anchor) = self.payload_anchors.try_allocate() else {
                return false;
            };
            payloads.insert(payload.anchor, anchor);
        }
        let owner = |old: AnchorId| groups.get(&old).copied().unwrap_or(AnchorId::INVALID);
        for group in &mut subtree.groups {
            group.anchor = owner(group.anchor);
            if group.parent_anchor.is_valid() {
                group.parent_anchor = owner(group.parent_anchor);
            }
            if let Some(scope) = &group.scope {
                scope.set_group_anchor(group.anchor);
            }
        }
        for payload in &mut subtree.payloads {
            payload.anchor = payloads
                .get(&payload.anchor)
                .copied()
                .unwrap_or(PayloadAnchor::INVALID);
        }
        for node in &mut subtree.nodes {
            node.owner = owner(node.owner);
        }
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MovableAttachment {
    AttachedHere,
    AttachedElsewhereInHost,
    NotAttachedHere,
}

impl SlotWriteSession<'_> {
    pub(crate) fn movable_attachment(&mut self, key: GroupKey) -> MovableAttachment {
        let Some(id) = key.movable_id() else {
            return MovableAttachment::NotAttachedHere;
        };
        // The content may sit among the groups an insert run holds aside.
        self.table.settle_insert_run();
        match self.table.movable_parent_anchor(id) {
            Some(parent) if parent == self.state.current_parent_anchor() => {
                MovableAttachment::AttachedHere
            }
            Some(_) => MovableAttachment::AttachedElsewhereInHost,
            None => MovableAttachment::NotAttachedHere,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplitMovables {
    All,
    SkipSoleOwnedInSubtree,
}

const INLINE_OWNED_MOVABLES: usize = 16;

#[derive(Default)]
struct SoleOwnedMovables {
    entries: SmallVec<[(Key, bool); INLINE_OWNED_MOVABLES]>,
    collected: bool,
    kept_any: bool,
}

impl SoleOwnedMovables {
    fn lookup(&self, id: Key) -> Result<usize, usize> {
        self.entries.binary_search_by_key(&id, |&(key, _)| key)
    }

    #[cold]
    #[inline(never)]
    fn ensure_collected(&mut self, payloads: &[super::PayloadRecord]) {
        if self.collected {
            return;
        }
        self.collected = true;
        for payload in payloads {
            if let Some(id) = sole_owned_movable_id(&*payload.value)
                && let Err(index) = self.lookup(id)
            {
                self.entries.insert(index, (id, false));
            }
        }
    }

    fn mark_kept(&mut self, id: Key) -> bool {
        let Ok(index) = self.lookup(id) else {
            return false;
        };
        self.entries[index].1 = true;
        self.kept_any = true;
        true
    }

    fn was_kept(&self, id: Key) -> bool {
        self.lookup(id).is_ok_and(|index| self.entries[index].1)
    }

    fn any_kept(&self) -> bool {
        self.kept_any
    }
}

impl DetachedSubtree {
    pub(crate) fn split_off_nested_movables(&mut self) -> Vec<Self> {
        self.split_off_nested_movables_with(SplitMovables::All)
    }

    // Splits off nested movables that may still outlive this disposed subtree.
    // Movables whose sole owner lives in this subtree with no external handles
    // stay in place and dispose directly with the subtree, avoiding a split,
    // retain, and re-disposal round-trip.
    pub(crate) fn split_off_unowned_nested_movables(&mut self) -> Vec<Self> {
        self.split_off_nested_movables_with(SplitMovables::SkipSoleOwnedInSubtree)
    }

    fn split_off_nested_movables_with(&mut self, mode: SplitMovables) -> Vec<Self> {
        let Some(offset) = self
            .groups
            .get(1..)
            .and_then(|rest| rest.iter().position(|group| group.key.is_movable()))
        else {
            return Vec::new();
        };
        self.split_off_nested_movables_from(offset + 1, mode)
    }

    #[cold]
    #[inline(never)]
    fn split_off_nested_movables_from(&mut self, start: usize, mode: SplitMovables) -> Vec<Self> {
        let mut split = Vec::new();
        let mut owned = SoleOwnedMovables::default();
        let mut index = start;
        while index < self.groups.len() {
            let group = &self.groups[index];
            let Some(movable_id) = group.key.movable_id() else {
                index += 1;
                continue;
            };
            if mode == SplitMovables::SkipSoleOwnedInSubtree {
                owned.ensure_collected(&self.payloads);
                if owned.mark_kept(movable_id) {
                    // Keep this movable inside the disposed subtree, but continue
                    // scanning inside it so any nested unowned movables still split.
                    index += 1;
                    continue;
                }
            }
            let len = group.subtree_len as usize;
            let Some(end) = index
                .checked_add(len)
                .filter(|end| *end <= self.groups.len())
            else {
                log::error!(
                    "detached subtree stopped splitting movables at group {index} with span {len} past {} groups",
                    self.groups.len()
                );
                break;
            };
            if len == 0 {
                log::error!("detached subtree skipped a movable group with an empty span");
                index += 1;
                continue;
            }
            split.push(self.split_range(index, end));
        }
        if owned.any_kept() {
            for payload in &self.payloads {
                if let Some(id) = sole_owned_movable_id(&*payload.value)
                    && owned.was_kept(id)
                {
                    disarm_remembered_movable(&*payload.value);
                }
            }
        }
        split
    }

    #[cold]
    #[inline(never)]
    fn split_range(&mut self, index: usize, end: usize) -> Self {
        let mut groups = self.groups.drain(index..end).collect::<Vec<_>>();
        let payloads = extract_subtree_segment::<PayloadSegment, _>(
            &mut self.groups,
            &mut self.payloads,
            index,
            &mut groups,
        );
        let nodes = extract_subtree_segment::<NodeSegment, _>(
            &mut self.groups,
            &mut self.nodes,
            index,
            &mut groups,
        );
        let root_depth = groups[0].depth;
        for group in &mut groups {
            group.depth = group
                .depth
                .checked_sub(root_depth)
                .expect("group depth underflow");
        }
        groups[0].parent_anchor = AnchorId::INVALID;
        let span = u32::try_from(groups.len()).expect("split movable span overflow");
        let node_count = u32::try_from(nodes.len()).expect("split movable node count overflow");
        self.shrink_ancestors_before(index, root_depth, span, node_count);
        let subtree = Self {
            groups,
            payloads,
            nodes,
        };
        #[cfg(any(test, debug_assertions))]
        subtree
            .validate_detached()
            .expect("split movable subtree must validate after extraction");
        subtree
    }

    fn shrink_ancestors_before(&mut self, index: usize, depth: u32, span: u32, nodes: u32) {
        let mut want = depth;
        for group in self.groups[..index].iter_mut().rev() {
            if want == 0 {
                break;
            }
            if group.depth != want - 1 {
                continue;
            }
            group.subtree_len = group
                .subtree_len
                .checked_sub(span)
                .filter(|&len| len >= 1)
                .expect("detached group subtree span underflow");
            group.subtree_node_count = group
                .subtree_node_count
                .checked_sub(nodes)
                .expect("detached group subtree node count underflow");
            want -= 1;
        }
    }
}

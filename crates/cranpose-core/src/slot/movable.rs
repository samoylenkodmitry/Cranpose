//! Movable content: groups whose identity is a key alone, not a call site.
//!
//! A movable group can be emitted from any parent in a slot table and keep
//! its payloads, scopes and nodes. The table indexes every attached movable
//! by its key so a composing site can tell whether the content is still
//! attached elsewhere, and a detached subtree can give up the movables
//! nested inside it so each one is retained under its own identity.

use super::{
    CheckedU32Delta, DetachedSubtree, GroupKey, GroupRecord, SlotTable, SlotWriteSession,
    checked_u32_delta, checked_usize_to_i64,
    segments::{NodeSegment, PayloadSegment, extract_subtree_segment},
};
use crate::{AnchorId, Key, collections::map::HashMap};

pub(crate) const MOVABLE_STATIC_KEY: Key = 0x6d6f_7661_626c_6521;

pub(crate) const MOVABLE_PLACEHOLDER_STATIC_KEY: Key = 0x6d6f_7661_626c_6520;

impl GroupKey {
    pub(crate) fn is_movable(self) -> bool {
        self.static_key == MOVABLE_STATIC_KEY && self.explicit_key.is_some()
    }

    pub(crate) fn movable_id(self) -> Option<Key> {
        self.is_movable().then_some(self.explicit_key).flatten()
    }
}

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
}

impl SlotWriteSession<'_> {
    pub(crate) fn movable_attached_elsewhere(&self, key: GroupKey) -> bool {
        let Some(id) = key.movable_id() else {
            return false;
        };
        self.table
            .movable_parent_anchor(id)
            .is_some_and(|parent| parent != self.state.current_parent_anchor())
    }
}

impl DetachedSubtree {
    pub(crate) fn split_off_nested_movables(&mut self) -> Vec<DetachedSubtree> {
        let mut split = Vec::new();
        let mut index = 1;
        while index < self.groups.len() {
            let group = &self.groups[index];
            if !group.key.is_movable() {
                index += 1;
                continue;
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
        split
    }

    fn split_range(&mut self, index: usize, end: usize) -> DetachedSubtree {
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
        let depth_delta = CheckedU32Delta::from_i64(-i64::from(root_depth), "group depth");
        for group in &mut groups {
            group.depth = checked_u32_delta(group.depth, depth_delta, 0, "group depth");
        }
        groups[0].parent_anchor = AnchorId::INVALID;
        self.shrink_ancestors_before(
            index,
            root_depth,
            checked_usize_to_i64(groups.len(), "split movable span"),
            checked_usize_to_i64(nodes.len(), "split movable node count"),
        );
        let subtree = DetachedSubtree {
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

    fn shrink_ancestors_before(&mut self, index: usize, depth: u32, span: i64, nodes: i64) {
        let span = CheckedU32Delta::from_i64(-span, "detached group subtree span");
        let nodes = CheckedU32Delta::from_i64(-nodes, "detached group subtree node count");
        let mut want = depth;
        for group in self.groups[..index].iter_mut().rev() {
            if want == 0 {
                break;
            }
            if group.depth != want - 1 {
                continue;
            }
            group.subtree_len =
                checked_u32_delta(group.subtree_len, span, 1, "detached group subtree span");
            group.subtree_node_count = checked_u32_delta(
                group.subtree_node_count,
                nodes,
                0,
                "detached group subtree node count",
            );
            want -= 1;
        }
    }
}

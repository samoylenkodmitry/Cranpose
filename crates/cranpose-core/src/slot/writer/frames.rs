use super::siblings::SiblingIndex;
use crate::{
    collections::map::{HashMap, HashSet},
    slot::GroupKey,
    AnchorId, Key,
};

#[derive(Default)]
pub(in crate::slot) struct RootFrame {
    pub(in crate::slot) next_child_index: usize,
    pub(in crate::slot) detach_remaining_children: bool,
    pub(in crate::slot) key_ordinals: HashMap<Key, u32>,
    pub(in crate::slot) seen_group_keys: HashSet<GroupKey>,
    pub(in crate::slot) sibling_index: Option<SiblingIndex>,
}

pub(in crate::slot) struct GroupFrame {
    pub(in crate::slot) group_anchor: AnchorId,
    pub(in crate::slot) next_child_index: usize,
    pub(in crate::slot) payload_cursor: usize,
    pub(in crate::slot) old_payload_len: usize,
    pub(in crate::slot) node_cursor: usize,
    pub(in crate::slot) old_node_len: usize,
    pub(in crate::slot) key_ordinals: HashMap<Key, u32>,
    pub(in crate::slot) seen_group_keys: HashSet<GroupKey>,
    pub(in crate::slot) sibling_index: Option<SiblingIndex>,
    pub(in crate::slot) body_finished: bool,
    pub(in crate::slot) was_skipped: bool,
}

impl GroupFrame {
    pub(in crate::slot) fn mark_body_finished(&mut self) -> bool {
        if self.body_finished {
            return false;
        }
        self.body_finished = true;
        true
    }

    pub(in crate::slot) fn advance_payload_cursor(&mut self) {
        self.payload_cursor += 1;
    }

    pub(in crate::slot) fn advance_node_cursor(&mut self) {
        self.node_cursor += 1;
    }

    pub(in crate::slot) fn skip_to_existing_group_end(
        &mut self,
        group_index: usize,
        subtree_len: usize,
    ) {
        self.next_child_index = group_index + subtree_len;
        self.payload_cursor = self.old_payload_len;
        self.node_cursor = self.old_node_len;
        self.was_skipped = true;
    }
}

use super::siblings::SiblingIndex;
use crate::{
    collections::map::{HashMap, HashSet},
    slot_storage::GroupKey,
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

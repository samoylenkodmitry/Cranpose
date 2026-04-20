use crate::{
    collections::map::HashMap,
    remove_child_and_cleanup_now,
    slot_storage::{
        GroupId, GroupKey, GroupKeySeed, GroupStartKind, StartScopedGroup, ValueSlotId,
    },
    AnchorId, Applier, Key, NodeId, Owned, ScopeId,
};
use std::any::Any;
use std::mem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableDebugStats {
    pub slots_len: usize,
    pub slots_cap: usize,
    pub pending_slot_drops_len: usize,
    pub pending_slot_drops_cap: usize,
    pub anchors_len: usize,
    pub anchors_cap: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotValueTypeDebugStat {
    pub type_name: &'static str,
    pub count: usize,
    pub inline_payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotPassMode {
    Compose,
    Recompose,
}

struct GroupRecord {
    key: GroupKey,
    parent_anchor: AnchorId,
    depth: u32,
    subtree_len: u32,
    subtree_node_count: u32,
    generation: u32,
    anchor: AnchorId,
    scope_id: Option<ScopeId>,
    payloads: Vec<PayloadRecord>,
    nodes: Vec<NodeRecord>,
}

struct PayloadRecord {
    owner: AnchorId,
    anchor: usize,
    generation: u32,
    type_name: &'static str,
    inline_payload_bytes: usize,
    value: Box<dyn Any>,
}

#[derive(Clone, Copy)]
struct NodeRecord {
    owner: AnchorId,
    id: NodeId,
    generation: u32,
}

pub(crate) struct DetachedSubtree {
    root_key: GroupKey,
    root_scope_id: Option<ScopeId>,
    groups: Vec<GroupRecord>,
}

impl DetachedSubtree {
    pub(crate) fn root_key(&self) -> GroupKey {
        self.root_key
    }

    pub(crate) fn root_scope_id(&self) -> Option<ScopeId> {
        self.root_scope_id
    }

    pub(crate) fn node_ids(&self) -> Vec<NodeId> {
        self.groups
            .iter()
            .flat_map(|group| group.nodes.iter().map(|node| node.id))
            .collect()
    }

    pub(crate) fn node_count(&self) -> usize {
        self.groups
            .first()
            .map(|group| group.subtree_node_count as usize)
            .unwrap_or(0)
    }

    pub(crate) fn group_count(&self) -> usize {
        self.groups.len()
    }

    pub(crate) fn scope_ids(&self) -> Vec<ScopeId> {
        self.groups
            .iter()
            .filter_map(|group| group.scope_id)
            .collect()
    }
}

pub(crate) struct FinishGroupResult {
    pub(crate) detached_children: Vec<DetachedSubtree>,
    pub(crate) structure_changed: bool,
    pub(crate) direct_nodes: Vec<NodeId>,
    pub(crate) subtree_nodes: Vec<NodeId>,
}

pub(crate) enum DeferredDrop {
    Value(Box<dyn Any>),
}

impl DeferredDrop {
    fn dispose(self) {
        match self {
            Self::Value(value) => drop(value),
        }
    }
}

#[derive(Default)]
pub(crate) struct SlotLifecycleCoordinator {
    pending_drops: Vec<DeferredDrop>,
}

impl SlotLifecycleCoordinator {
    pub(crate) fn queue_drop(&mut self, drop: DeferredDrop) {
        self.pending_drops.push(drop);
    }

    pub(crate) fn queue_subtree_disposal(&mut self, subtree: DetachedSubtree) {
        for group in subtree.groups.into_iter().rev() {
            for payload in group.payloads.into_iter().rev() {
                self.queue_drop(DeferredDrop::Value(payload.value));
            }
        }
    }

    pub(crate) fn flush_pending_drops(&mut self) {
        while let Some(drop) = self.pending_drops.pop() {
            drop.dispose();
        }
    }

    pub(crate) fn compact_storage(&mut self) {
        if self.pending_drops.is_empty() {
            self.pending_drops.shrink_to_fit();
        }
    }

    pub(crate) fn pending_drops_len(&self) -> usize {
        self.pending_drops.len()
    }

    pub(crate) fn pending_drops_capacity(&self) -> usize {
        self.pending_drops.capacity()
    }

    pub(crate) fn fill_debug_stats(&self, stats: &mut SlotTableDebugStats) {
        stats.pending_slot_drops_len = self.pending_drops_len();
        stats.pending_slot_drops_cap = self.pending_drops_capacity();
    }

    pub(crate) fn dispose_slot_table(&mut self, table: &mut SlotTable) {
        self.flush_pending_drops();
        let drops = table.take_all_drops();
        self.pending_drops.extend(drops);
        self.flush_pending_drops();
    }
}

#[derive(Default)]
struct RootFrame {
    old_children: Vec<AnchorId>,
    old_cursor: usize,
    next_child_index: usize,
    key_ordinals: HashMap<Key, u32>,
}

struct GroupFrame {
    group_anchor: AnchorId,
    start_kind: GroupStartKind,
    old_children: Vec<AnchorId>,
    old_cursor: usize,
    next_child_index: usize,
    payload_cursor: usize,
    old_payload_len: usize,
    node_cursor: usize,
    old_node_len: usize,
    key_ordinals: HashMap<Key, u32>,
    body_finished: bool,
}

#[derive(Default)]
pub(crate) struct SlotWriteSessionState {
    root: RootFrame,
    group_stack: Vec<GroupFrame>,
    removed_node_count: usize,
    removed_group_count: usize,
    pub(crate) request_compaction: bool,
}

impl SlotWriteSessionState {
    const COMPACT_NODE_THRESHOLD: usize = 16 * 1024;
    const COMPACT_GROUP_THRESHOLD: usize = 32 * 1024;

    pub(crate) fn reset_for_pass(&mut self, table: &SlotTable, mode: SlotPassMode) {
        self.root = RootFrame {
            old_children: table.direct_child_anchors(AnchorId::INVALID),
            old_cursor: 0,
            next_child_index: 0,
            key_ordinals: HashMap::default(),
        };
        self.group_stack.clear();
        self.removed_node_count = 0;
        self.removed_group_count = 0;
        self.request_compaction = false;
        if matches!(mode, SlotPassMode::Recompose) {
            self.root.old_children.clear();
        }
    }

    fn note_removed_nodes(&mut self, count: usize) {
        self.removed_node_count += count;
        self.update_compaction_hint();
    }

    fn note_detached_subtrees(&mut self, subtrees: &[DetachedSubtree]) {
        self.removed_group_count += subtrees
            .iter()
            .map(DetachedSubtree::group_count)
            .sum::<usize>();
        self.removed_node_count += subtrees
            .iter()
            .map(DetachedSubtree::node_count)
            .sum::<usize>();
        self.update_compaction_hint();
    }

    fn update_compaction_hint(&mut self) {
        self.request_compaction |= self.removed_node_count >= Self::COMPACT_NODE_THRESHOLD
            || self.removed_group_count >= Self::COMPACT_GROUP_THRESHOLD;
    }
}

pub(crate) struct SlotWriteSession<'a> {
    table: &'a mut SlotTable,
    lifecycle: &'a mut SlotLifecycleCoordinator,
    state: &'a mut SlotWriteSessionState,
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlotInvariantError {
    GroupAnchorCountMismatch {
        expected: usize,
        actual: usize,
    },
    GroupAnchorIndexMismatch {
        anchor: AnchorId,
        expected: usize,
        actual: Option<usize>,
    },
    InvalidParent {
        group_index: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    BadDepth {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    BadSubtreeLen {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    BadSubtreeNodeCount {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    PayloadAnchorCountMismatch {
        expected: usize,
        actual: usize,
    },
    ScopeIndexCountMismatch {
        expected: usize,
        actual: usize,
    },
    PayloadOwnerMismatch {
        payload_anchor: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    PayloadLocationMismatch {
        payload_anchor: usize,
        expected: (AnchorId, usize),
        actual: Option<(AnchorId, usize)>,
    },
    NodeOwnerMismatch {
        node_id: NodeId,
        expected: AnchorId,
        actual: AnchorId,
    },
    DuplicateSiblingKey {
        parent_anchor: AnchorId,
        key: GroupKey,
    },
    ScopeIndexMismatch {
        scope_id: ScopeId,
        expected: AnchorId,
        actual: Option<AnchorId>,
    },
}

pub struct SlotTable {
    groups: Vec<GroupRecord>,
    group_anchor_to_index: HashMap<AnchorId, usize>,
    payload_anchor_to_location: HashMap<usize, (AnchorId, usize)>,
    scope_anchor_to_group: HashMap<ScopeId, AnchorId>,
    next_group_anchor: usize,
    next_payload_anchor: usize,
}

impl SlotTable {
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            group_anchor_to_index: HashMap::default(),
            payload_anchor_to_location: HashMap::default(),
            scope_anchor_to_group: HashMap::default(),
            next_group_anchor: 1,
            next_payload_anchor: 1,
        }
    }

    pub fn heap_bytes(&self) -> usize {
        self.groups.capacity() * mem::size_of::<GroupRecord>()
            + self
                .groups
                .iter()
                .map(|group| group.payloads.capacity() * mem::size_of::<PayloadRecord>())
                .sum::<usize>()
            + self
                .groups
                .iter()
                .map(|group| group.nodes.capacity() * mem::size_of::<NodeRecord>())
                .sum::<usize>()
            + self.group_anchor_to_index.capacity() * mem::size_of::<(AnchorId, usize)>()
            + self.payload_anchor_to_location.capacity()
                * mem::size_of::<(usize, (AnchorId, usize))>()
            + self.scope_anchor_to_group.capacity() * mem::size_of::<(ScopeId, AnchorId)>()
    }

    pub fn debug_stats(&self) -> SlotTableDebugStats {
        let payload_len = self
            .groups
            .iter()
            .map(|group| group.payloads.len())
            .sum::<usize>();
        let payload_cap = self
            .groups
            .iter()
            .map(|group| group.payloads.capacity())
            .sum::<usize>();
        let node_len = self
            .groups
            .iter()
            .map(|group| group.nodes.len())
            .sum::<usize>();
        let node_cap = self
            .groups
            .iter()
            .map(|group| group.nodes.capacity())
            .sum::<usize>();
        SlotTableDebugStats {
            slots_len: self.groups.len() + payload_len + node_len,
            slots_cap: self.groups.capacity() + payload_cap + node_cap,
            anchors_len: self.groups.len(),
            anchors_cap: self.group_anchor_to_index.capacity(),
            ..SlotTableDebugStats::default()
        }
    }

    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                (
                    index,
                    group.key.static_key,
                    group.scope_id,
                    group.subtree_len as usize,
                )
            })
            .collect()
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        let mut rows = Vec::new();
        for (index, group) in self.groups.iter().enumerate() {
            rows.push((
                index,
                format!(
                    "Group(key={:?}, scope={:?}, subtree_len={}, payload_len={}, node_len={})",
                    group.key,
                    group.scope_id,
                    group.subtree_len,
                    group.payloads.len(),
                    group.nodes.len()
                ),
            ));
        }
        let base = rows.len();
        let mut offset = 0usize;
        for group in &self.groups {
            for payload in &group.payloads {
                rows.push((
                    base + offset,
                    format!(
                        "Value(owner={:?}, type={})",
                        payload.owner, payload.type_name
                    ),
                ));
                offset += 1;
            }
        }
        let base = rows.len();
        let mut offset = 0usize;
        for group in &self.groups {
            for node in &group.nodes {
                rows.push((
                    base + offset,
                    format!(
                        "Node(owner={:?}, id={}, gen={})",
                        node.owner, node.id, node.generation
                    ),
                ));
                offset += 1;
            }
        }
        rows
    }

    pub fn debug_value_type_counts(&self, limit: usize) -> Vec<SlotValueTypeDebugStat> {
        let mut counts: HashMap<&'static str, (usize, usize)> = HashMap::default();
        for group in &self.groups {
            for payload in &group.payloads {
                let entry = counts.entry(payload.type_name).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += payload.inline_payload_bytes;
            }
        }
        let mut stats = counts
            .into_iter()
            .map(
                |(type_name, (count, inline_payload_bytes))| SlotValueTypeDebugStat {
                    type_name,
                    count,
                    inline_payload_bytes,
                },
            )
            .collect::<Vec<_>>();
        stats.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.type_name.cmp(right.type_name))
        });
        stats.truncate(limit);
        stats
    }

    pub(crate) fn write_session<'a>(
        &'a mut self,
        lifecycle: &'a mut SlotLifecycleCoordinator,
        state: &'a mut SlotWriteSessionState,
        _mode: SlotPassMode,
    ) -> SlotWriteSession<'a> {
        SlotWriteSession {
            table: self,
            lifecycle,
            state,
        }
    }

    pub(crate) fn read_value<T: 'static>(&self, slot: ValueSlotId) -> &T {
        let group = slot.group();
        let group_index = group.index();
        let record = self
            .groups
            .get(group_index)
            .expect("value slot group index missing");
        assert_eq!(
            record.generation,
            group.generation(),
            "value slot group generation mismatch"
        );
        let payload = record
            .payloads
            .get(slot.offset())
            .expect("value slot offset missing");
        assert_eq!(
            payload.generation,
            slot.generation(),
            "value slot generation mismatch"
        );
        payload
            .value
            .downcast_ref::<T>()
            .expect("value slot type mismatch")
    }

    pub(crate) fn read_value_mut<T: 'static>(&mut self, slot: ValueSlotId) -> &mut T {
        let group = slot.group();
        let group_index = group.index();
        let record = self
            .groups
            .get(group_index)
            .expect("value slot group index missing");
        assert_eq!(
            record.generation,
            group.generation(),
            "value slot group generation mismatch"
        );
        let payload = self.groups[group_index]
            .payloads
            .get_mut(slot.offset())
            .expect("value slot offset missing");
        assert_eq!(
            payload.generation,
            slot.generation(),
            "value slot generation mismatch"
        );
        payload
            .value
            .downcast_mut::<T>()
            .expect("value slot type mismatch")
    }

    pub(crate) fn write_value<T: 'static>(&mut self, slot: ValueSlotId, value: T) {
        let group = slot.group();
        let group_index = group.index();
        let owner = self.groups[group_index].anchor;
        let payload_anchor = self.groups[group_index]
            .payloads
            .get(slot.offset())
            .expect("value slot offset missing")
            .anchor;
        self.groups[group_index].payloads[slot.offset()] = PayloadRecord {
            owner,
            anchor: payload_anchor,
            generation: slot.generation(),
            type_name: std::any::type_name::<T>(),
            inline_payload_bytes: mem::size_of::<T>(),
            value: Box::new(value),
        };
    }

    pub(crate) fn debug_verify(&self, _lifecycle: Option<&SlotLifecycleCoordinator>) {
        #[cfg(debug_assertions)]
        if let Err(err) = self.validate() {
            panic!("slot table invariant violation: {err:?}");
        }
    }

    pub(crate) fn compact_storage(&mut self) {
        self.groups.shrink_to_fit();
        for group in &mut self.groups {
            group.payloads.shrink_to_fit();
            group.nodes.shrink_to_fit();
        }
        self.group_anchor_to_index.shrink_to_fit();
        self.payload_anchor_to_location.shrink_to_fit();
        self.scope_anchor_to_group.shrink_to_fit();
    }

    pub(crate) fn take_all_drops(&mut self) -> Vec<DeferredDrop> {
        let payload_count = self
            .groups
            .iter()
            .map(|group| group.payloads.len())
            .sum::<usize>();
        let mut drops = Vec::with_capacity(self.groups.len() + payload_count);
        for group in self.groups.drain(..).rev() {
            for payload in group.payloads.into_iter().rev() {
                drops.push(DeferredDrop::Value(payload.value));
            }
        }
        self.group_anchor_to_index.clear();
        self.payload_anchor_to_location.clear();
        self.scope_anchor_to_group.clear();
        drops
    }

    fn direct_child_anchors(&self, parent_anchor: AnchorId) -> Vec<AnchorId> {
        let mut children = Vec::new();
        if !parent_anchor.is_valid() {
            let mut index = 0;
            while index < self.groups.len() {
                let group = &self.groups[index];
                debug_assert!(
                    !group.parent_anchor.is_valid(),
                    "root-level traversal should only step through root groups"
                );
                children.push(group.anchor);
                index += group.subtree_len as usize;
            }
            return children;
        }

        let parent_index = self.current_group_index(parent_anchor);
        let end = parent_index + self.groups[parent_index].subtree_len as usize;
        let mut index = parent_index + 1;
        while index < end {
            let group = &self.groups[index];
            debug_assert_eq!(
                group.parent_anchor, parent_anchor,
                "direct child traversal must land on immediate children only"
            );
            children.push(group.anchor);
            index += group.subtree_len as usize;
        }
        children
    }

    fn current_group_index(&self, anchor: AnchorId) -> usize {
        *self
            .group_anchor_to_index
            .get(&anchor)
            .expect("group anchor should resolve")
    }

    fn current_group(&self, anchor: AnchorId) -> &GroupRecord {
        &self.groups[self.current_group_index(anchor)]
    }

    fn allocate_group_anchor(&mut self) -> AnchorId {
        let anchor = AnchorId::new(self.next_group_anchor);
        self.next_group_anchor += 1;
        anchor
    }

    fn allocate_payload_anchor(&mut self) -> usize {
        let anchor = self.next_payload_anchor;
        self.next_payload_anchor += 1;
        anchor
    }

    fn recompute_group_indexes(&mut self) {
        self.group_anchor_to_index.clear();
        self.scope_anchor_to_group.clear();
        for (index, group) in self.groups.iter().enumerate() {
            self.group_anchor_to_index.insert(group.anchor, index);
            if let Some(scope_id) = group.scope_id {
                self.scope_anchor_to_group.insert(scope_id, group.anchor);
            }
        }
    }

    fn recompute_group_spans(&mut self) {
        for group in &mut self.groups {
            group.subtree_len = 1;
            group.subtree_node_count = group.nodes.len() as u32;
        }

        for index in (0..self.groups.len()).rev() {
            let parent_anchor = self.groups[index].parent_anchor;
            if parent_anchor.is_valid() {
                if let Some(parent_index) = self.group_anchor_to_index.get(&parent_anchor).copied()
                {
                    let subtree_len = self.groups[index].subtree_len;
                    let subtree_nodes = self.groups[index].subtree_node_count;
                    self.groups[parent_index].subtree_len += subtree_len;
                    self.groups[parent_index].subtree_node_count += subtree_nodes;
                }
            }
        }
    }

    fn recompute_all_metadata(&mut self) {
        self.recompute_group_indexes();
        self.recompute_group_spans();
    }

    fn refresh_group_indexes_from(&mut self, start: usize) {
        for index in start..self.groups.len() {
            self.group_anchor_to_index
                .insert(self.groups[index].anchor, index);
        }
    }

    fn clear_group_indexes(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            self.group_anchor_to_index.remove(&group.anchor);
            if let Some(scope_id) = group.scope_id {
                self.scope_anchor_to_group.remove(&scope_id);
            }
        }
    }

    fn adjust_ancestor_group_spans(
        &mut self,
        parent_anchor: AnchorId,
        subtree_delta: i32,
        node_delta: i32,
    ) {
        let mut current = parent_anchor;
        while current.is_valid() {
            let group_index = self.current_group_index(current);
            let group = &mut self.groups[group_index];
            let subtree_len = group.subtree_len as i32 + subtree_delta;
            let subtree_nodes = group.subtree_node_count as i32 + node_delta;
            debug_assert!(
                subtree_len >= 1,
                "active groups must keep a positive subtree span"
            );
            debug_assert!(
                subtree_nodes >= 0,
                "subtree node counts cannot become negative"
            );
            group.subtree_len = subtree_len as u32;
            group.subtree_node_count = subtree_nodes as u32;
            current = group.parent_anchor;
        }
    }

    fn refresh_group_payload_locations(&mut self, owner: AnchorId, start: usize) {
        let group_index = self.current_group_index(owner);
        for (index, payload) in self.groups[group_index]
            .payloads
            .iter()
            .enumerate()
            .skip(start)
        {
            self.payload_anchor_to_location
                .insert(payload.anchor, (owner, index));
        }
    }

    fn rebuild_payload_locations_for_group_range(&mut self, start: usize, end: usize) {
        for group in &self.groups[start..end] {
            for (index, payload) in group.payloads.iter().enumerate() {
                self.payload_anchor_to_location
                    .insert(payload.anchor, (group.anchor, index));
            }
            if let Some(scope_id) = group.scope_id {
                self.scope_anchor_to_group.insert(scope_id, group.anchor);
            }
        }
    }

    fn insert_payload_record(
        &mut self,
        owner: AnchorId,
        insert_index: usize,
        record: PayloadRecord,
    ) {
        let owner_index = self.current_group_index(owner);
        self.groups[owner_index]
            .payloads
            .insert(insert_index, record);
        self.refresh_group_payload_locations(owner, insert_index);
    }

    fn remove_payload_range(
        &mut self,
        owner: AnchorId,
        start: usize,
        end: usize,
    ) -> Vec<PayloadRecord> {
        if start >= end {
            return Vec::new();
        }
        let owner_index = self.current_group_index(owner);
        let removed = self.groups[owner_index]
            .payloads
            .drain(start..end)
            .collect::<Vec<_>>();
        for payload in &removed {
            self.payload_anchor_to_location.remove(&payload.anchor);
        }
        self.refresh_group_payload_locations(owner, start);
        removed
    }

    fn adjust_ancestor_node_counts(&mut self, owner: AnchorId, delta: i32) {
        let mut current = Some(owner);
        while let Some(anchor) = current {
            let group_index = self.current_group_index(anchor);
            let group = &mut self.groups[group_index];
            let updated = group.subtree_node_count as i32 + delta;
            debug_assert!(updated >= 0, "subtree node counts cannot become negative");
            group.subtree_node_count = updated as u32;
            current = group
                .parent_anchor
                .is_valid()
                .then_some(group.parent_anchor);
        }
    }

    fn next_key_ordinal(map: &mut HashMap<Key, u32>, key: Key) -> u32 {
        let ordinal = map.get(&key).copied().unwrap_or(0);
        map.insert(key, ordinal + 1);
        ordinal
    }

    fn expected_key_ordinal(map: &HashMap<Key, u32>, key: Key) -> u32 {
        map.get(&key).copied().unwrap_or(0)
    }

    fn consume_group_key(state: &mut SlotWriteSessionState, key: GroupKey) {
        let ordinal = key.explicit_key.map_or_else(
            || SlotTable::next_key_ordinal(SlotTable::current_key_ordinals(state), key.static_key),
            |_| 0,
        );
        debug_assert_eq!(
            ordinal, key.ordinal,
            "reserved group ordinal must match the active writer state"
        );
    }

    fn current_parent_anchor(state: &SlotWriteSessionState) -> AnchorId {
        state
            .group_stack
            .last()
            .map(|frame| frame.group_anchor)
            .unwrap_or(AnchorId::INVALID)
    }

    fn current_old_children(state: &mut SlotWriteSessionState) -> (&mut Vec<AnchorId>, &mut usize) {
        if let Some(frame) = state.group_stack.last_mut() {
            (&mut frame.old_children, &mut frame.old_cursor)
        } else {
            (&mut state.root.old_children, &mut state.root.old_cursor)
        }
    }

    fn current_next_child_index(state: &mut SlotWriteSessionState) -> &mut usize {
        if let Some(frame) = state.group_stack.last_mut() {
            &mut frame.next_child_index
        } else {
            &mut state.root.next_child_index
        }
    }

    fn current_key_ordinals(state: &mut SlotWriteSessionState) -> &mut HashMap<Key, u32> {
        if let Some(frame) = state.group_stack.last_mut() {
            &mut frame.key_ordinals
        } else {
            &mut state.root.key_ordinals
        }
    }

    fn current_key_ordinals_ref(state: &SlotWriteSessionState) -> &HashMap<Key, u32> {
        if let Some(frame) = state.group_stack.last() {
            &frame.key_ordinals
        } else {
            &state.root.key_ordinals
        }
    }

    fn insert_new_group(
        &mut self,
        insert_index: usize,
        parent_anchor: AnchorId,
        key: GroupKey,
    ) -> AnchorId {
        let depth = if parent_anchor.is_valid() {
            self.current_group(parent_anchor).depth + 1
        } else {
            0
        };
        let anchor = self.allocate_group_anchor();
        self.groups.insert(
            insert_index,
            GroupRecord {
                key,
                parent_anchor,
                depth,
                subtree_len: 1,
                subtree_node_count: 0,
                generation: 1,
                anchor,
                scope_id: None,
                payloads: Vec::new(),
                nodes: Vec::new(),
            },
        );
        self.refresh_group_indexes_from(insert_index);
        self.adjust_ancestor_group_spans(parent_anchor, 1, 0);
        anchor
    }

    fn move_subtree(&mut self, anchor: AnchorId, insert_index: usize) {
        let from_index = self.current_group_index(anchor);
        if from_index == insert_index {
            return;
        }
        let subtree_len = self.groups[from_index].subtree_len as usize;
        let moved = self
            .groups
            .drain(from_index..from_index + subtree_len)
            .collect::<Vec<_>>();
        let adjusted_index = if insert_index > from_index {
            insert_index - subtree_len
        } else {
            insert_index
        };
        self.groups.splice(adjusted_index..adjusted_index, moved);
        self.refresh_group_indexes_from(from_index.min(adjusted_index));
    }

    fn open_group_frame(
        &mut self,
        state: &mut SlotWriteSessionState,
        anchor: AnchorId,
        start_kind: GroupStartKind,
    ) -> usize {
        let group_index = self.current_group_index(anchor);
        let group = &self.groups[group_index];
        state.group_stack.push(GroupFrame {
            group_anchor: anchor,
            start_kind,
            old_children: self.direct_child_anchors(anchor),
            old_cursor: 0,
            next_child_index: group_index + 1,
            payload_cursor: 0,
            old_payload_len: group.payloads.len(),
            node_cursor: 0,
            old_node_len: group.nodes.len(),
            key_ordinals: HashMap::default(),
            body_finished: false,
        });
        group_index
    }

    fn detach_subtree(&mut self, anchor: AnchorId) -> DetachedSubtree {
        let root_index = self.current_group_index(anchor);
        let root_key = self.groups[root_index].key;
        let root_scope_id = self.groups[root_index].scope_id;
        let root_parent_anchor = self.groups[root_index].parent_anchor;
        let root_subtree_len = self.groups[root_index].subtree_len;
        let root_subtree_node_count = self.groups[root_index].subtree_node_count;
        let subtree_len = root_subtree_len as usize;
        let removed_groups = self
            .groups
            .drain(root_index..root_index + subtree_len)
            .collect::<Vec<_>>();
        self.clear_group_indexes(&removed_groups);
        self.refresh_group_indexes_from(root_index);
        for group in &removed_groups {
            for payload in &group.payloads {
                self.payload_anchor_to_location.remove(&payload.anchor);
            }
        }

        self.adjust_ancestor_group_spans(
            root_parent_anchor,
            -(root_subtree_len as i32),
            -(root_subtree_node_count as i32),
        );

        DetachedSubtree {
            root_key,
            root_scope_id,
            groups: removed_groups,
        }
    }

    fn restore_subtree(
        &mut self,
        insert_index: usize,
        parent_anchor: AnchorId,
        key: GroupKey,
        mut subtree: DetachedSubtree,
    ) -> AnchorId {
        let root_anchor = subtree
            .groups
            .first()
            .map(|group| group.anchor)
            .expect("detached subtree must contain a root group");

        let depth_delta = if parent_anchor.is_valid() {
            self.current_group(parent_anchor).depth + 1
        } else {
            0
        } as i32
            - subtree.groups[0].depth as i32;

        subtree.groups[0].key = key;
        subtree.groups[0].parent_anchor = parent_anchor;
        for group in &mut subtree.groups {
            group.depth = ((group.depth as i32) + depth_delta) as u32;
        }

        let subtree_len = subtree.groups.len();
        self.groups
            .splice(insert_index..insert_index, subtree.groups);
        self.rebuild_payload_locations_for_group_range(insert_index, insert_index + subtree_len);
        self.recompute_all_metadata();
        root_anchor
    }

    fn finish_group_body_internal(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        state: &mut SlotWriteSessionState,
    ) -> FinishGroupResult {
        let frame = state
            .group_stack
            .last_mut()
            .expect("finish_group_body requires an active group");
        if frame.body_finished {
            return FinishGroupResult {
                detached_children: Vec::new(),
                structure_changed: false,
                direct_nodes: Vec::new(),
                subtree_nodes: Vec::new(),
            };
        }

        let group_anchor = frame.group_anchor;
        let payload_len = self.current_group(group_anchor).payloads.len();
        if frame.payload_cursor < payload_len {
            let removed =
                self.remove_payload_range(group_anchor, frame.payload_cursor, payload_len);
            for payload in removed {
                lifecycle.queue_drop(DeferredDrop::Value(payload.value));
            }
        }

        let mut direct_nodes = Vec::new();
        let removed = {
            let group_index = self.current_group_index(group_anchor);
            let group = &mut self.groups[group_index];
            if frame.node_cursor < group.nodes.len() {
                group.nodes.drain(frame.node_cursor..).collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        if !removed.is_empty() {
            self.adjust_ancestor_node_counts(group_anchor, -(removed.len() as i32));
            direct_nodes.extend(removed.into_iter().map(|node| node.id));
        }

        let mut detached_children = Vec::new();
        let remaining = frame.old_children[frame.old_cursor..].to_vec();
        for anchor in remaining.into_iter().rev() {
            if self.group_anchor_to_index.contains_key(&anchor) {
                detached_children.push(self.detach_subtree(anchor));
            }
        }
        detached_children.reverse();
        let subtree_nodes = detached_children
            .iter()
            .flat_map(DetachedSubtree::node_ids)
            .collect::<Vec<_>>();
        let structure_changed = !matches!(frame.start_kind, GroupStartKind::Reused)
            || !direct_nodes.is_empty()
            || !detached_children.is_empty();
        frame.body_finished = true;
        let _ = frame;
        state.note_removed_nodes(direct_nodes.len());
        state.note_detached_subtrees(&detached_children);
        FinishGroupResult {
            detached_children,
            structure_changed,
            direct_nodes,
            subtree_nodes,
        }
    }

    fn root_finish_result(&mut self, state: &mut SlotWriteSessionState) -> Vec<DetachedSubtree> {
        let remaining = state.root.old_children[state.root.old_cursor..].to_vec();
        let mut detached = Vec::new();
        for anchor in remaining.into_iter().rev() {
            if self.group_anchor_to_index.contains_key(&anchor) {
                detached.push(self.detach_subtree(anchor));
            }
        }
        detached.reverse();
        detached
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn validate(&self) -> Result<(), SlotInvariantError> {
        if self.group_anchor_to_index.len() != self.groups.len() {
            return Err(SlotInvariantError::GroupAnchorCountMismatch {
                expected: self.groups.len(),
                actual: self.group_anchor_to_index.len(),
            });
        }

        let mut stack: Vec<(AnchorId, usize)> = Vec::new();
        let mut sibling_keys: HashMap<(AnchorId, GroupKey), usize> = HashMap::default();
        let payload_count = self
            .groups
            .iter()
            .map(|group| group.payloads.len())
            .sum::<usize>();
        let scope_count = self
            .groups
            .iter()
            .filter(|group| group.scope_id.is_some())
            .count();

        for (index, group) in self.groups.iter().enumerate() {
            match self.group_anchor_to_index.get(&group.anchor).copied() {
                Some(actual) if actual == index => {}
                actual => {
                    return Err(SlotInvariantError::GroupAnchorIndexMismatch {
                        anchor: group.anchor,
                        expected: index,
                        actual,
                    });
                }
            }

            while let Some((_, end)) = stack.last() {
                if *end <= index {
                    stack.pop();
                } else {
                    break;
                }
            }

            let expected_parent = stack
                .last()
                .map(|(anchor, _)| *anchor)
                .unwrap_or(AnchorId::INVALID);
            if group.parent_anchor != expected_parent {
                return Err(SlotInvariantError::InvalidParent {
                    group_index: index,
                    expected: expected_parent,
                    actual: group.parent_anchor,
                });
            }

            let expected_depth = stack.len() as u32;
            if group.depth != expected_depth {
                return Err(SlotInvariantError::BadDepth {
                    group_index: index,
                    expected: expected_depth,
                    actual: group.depth,
                });
            }

            let subtree_end = index + group.subtree_len as usize;
            if subtree_end == index || subtree_end > self.groups.len() {
                return Err(SlotInvariantError::BadSubtreeLen {
                    group_index: index,
                    expected: 0,
                    actual: group.subtree_len,
                });
            }

            if sibling_keys
                .insert((group.parent_anchor, group.key), index)
                .is_some()
            {
                return Err(SlotInvariantError::DuplicateSiblingKey {
                    parent_anchor: group.parent_anchor,
                    key: group.key,
                });
            }

            for (payload_index, payload) in group.payloads.iter().enumerate() {
                if payload.owner != group.anchor {
                    return Err(SlotInvariantError::PayloadOwnerMismatch {
                        payload_anchor: payload.anchor,
                        expected: group.anchor,
                        actual: payload.owner,
                    });
                }
                let expected_location = (group.anchor, payload_index);
                let actual = self
                    .payload_anchor_to_location
                    .get(&payload.anchor)
                    .copied();
                if actual != Some(expected_location) {
                    return Err(SlotInvariantError::PayloadLocationMismatch {
                        payload_anchor: payload.anchor,
                        expected: expected_location,
                        actual,
                    });
                }
            }

            if let Some(scope_id) = group.scope_id {
                let actual = self.scope_anchor_to_group.get(&scope_id).copied();
                if actual != Some(group.anchor) {
                    return Err(SlotInvariantError::ScopeIndexMismatch {
                        scope_id,
                        expected: group.anchor,
                        actual,
                    });
                }
            }

            for node in &group.nodes {
                if node.owner != group.anchor {
                    return Err(SlotInvariantError::NodeOwnerMismatch {
                        node_id: node.id,
                        expected: group.anchor,
                        actual: node.owner,
                    });
                }
            }

            stack.push((group.anchor, subtree_end));
        }

        if self.payload_anchor_to_location.len() != payload_count {
            return Err(SlotInvariantError::PayloadAnchorCountMismatch {
                expected: payload_count,
                actual: self.payload_anchor_to_location.len(),
            });
        }

        if self.scope_anchor_to_group.len() != scope_count {
            return Err(SlotInvariantError::ScopeIndexCountMismatch {
                expected: scope_count,
                actual: self.scope_anchor_to_group.len(),
            });
        }

        for (&payload_anchor, &(owner, payload_index)) in &self.payload_anchor_to_location {
            let Some(group_index) = self.group_anchor_to_index.get(&owner).copied() else {
                return Err(SlotInvariantError::PayloadLocationMismatch {
                    payload_anchor,
                    expected: (owner, payload_index),
                    actual: None,
                });
            };
            let actual = self.groups[group_index]
                .payloads
                .get(payload_index)
                .map(|payload| (payload.owner, payload.anchor));
            if actual != Some((owner, payload_anchor)) {
                return Err(SlotInvariantError::PayloadLocationMismatch {
                    payload_anchor,
                    expected: (owner, payload_index),
                    actual: Some((owner, payload_index)),
                });
            }
        }

        let mut expected_subtree_len = vec![1u32; self.groups.len()];
        let mut expected_subtree_node_count = self
            .groups
            .iter()
            .map(|group| group.nodes.len() as u32)
            .collect::<Vec<_>>();
        for index in (0..self.groups.len()).rev() {
            let parent_anchor = self.groups[index].parent_anchor;
            if !parent_anchor.is_valid() {
                continue;
            }
            let parent_index = self
                .group_anchor_to_index
                .get(&parent_anchor)
                .copied()
                .expect("validated parents must resolve");
            expected_subtree_len[parent_index] += expected_subtree_len[index];
            expected_subtree_node_count[parent_index] += expected_subtree_node_count[index];
        }

        for (index, group) in self.groups.iter().enumerate() {
            if group.subtree_len != expected_subtree_len[index] {
                return Err(SlotInvariantError::BadSubtreeLen {
                    group_index: index,
                    expected: expected_subtree_len[index],
                    actual: group.subtree_len,
                });
            }
            if group.subtree_node_count != expected_subtree_node_count[index] {
                return Err(SlotInvariantError::BadSubtreeNodeCount {
                    group_index: index,
                    expected: expected_subtree_node_count[index],
                    actual: group.subtree_node_count,
                });
            }
        }

        Ok(())
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn preview_group_key(&self, seed: GroupKeySeed) -> GroupKey {
        let ordinal = seed.explicit_key.map_or_else(
            || {
                SlotTable::expected_key_ordinal(
                    SlotTable::current_key_ordinals_ref(self.state),
                    seed.static_key,
                )
            },
            |_| 0,
        );
        GroupKey::new(seed.static_key, seed.explicit_key, ordinal)
    }

    pub(crate) fn start_recompose_at_scope(&mut self, scope_id: ScopeId) -> Option<GroupId> {
        let anchor = self.table.scope_anchor_to_group.get(&scope_id).copied()?;
        let index = self.table.group_anchor_to_index.get(&anchor).copied()?;
        if self.table.groups[index].scope_id != Some(scope_id) {
            return None;
        }
        self.table
            .open_group_frame(self.state, anchor, GroupStartKind::Reused);
        Some(GroupId::new(index, self.table.groups[index].generation))
    }

    pub(crate) fn begin_scoped_group(
        &mut self,
        key: GroupKey,
        restored: Option<DetachedSubtree>,
    ) -> StartScopedGroup<GroupId> {
        SlotTable::consume_group_key(self.state, key);
        let parent_anchor = SlotTable::current_parent_anchor(self.state);
        let insert_index = *SlotTable::current_next_child_index(self.state);

        let (old_children, old_cursor) = SlotTable::current_old_children(self.state);
        let mut reused_position = None;
        let mut moved = false;
        if restored.is_none() {
            if let Some(&expected_anchor) = old_children.get(*old_cursor) {
                let expected = self.table.current_group(expected_anchor);
                if expected.parent_anchor == parent_anchor && expected.key == key {
                    reused_position = Some(*old_cursor);
                } else {
                    for position in (*old_cursor + 1)..old_children.len() {
                        let anchor = old_children[position];
                        let group = self.table.current_group(anchor);
                        if group.parent_anchor == parent_anchor && group.key == key {
                            old_children.remove(position);
                            old_children.insert(*old_cursor, anchor);
                            reused_position = Some(*old_cursor);
                            self.table.move_subtree(anchor, insert_index);
                            moved = true;
                            break;
                        }
                    }
                }
            }
        }

        let (anchor, kind) = if let Some(restored) = restored {
            (
                self.table
                    .restore_subtree(insert_index, parent_anchor, key, restored),
                GroupStartKind::Restored,
            )
        } else if reused_position.is_some() {
            let anchor = old_children[*old_cursor];
            *old_cursor += 1;
            (
                anchor,
                if moved {
                    GroupStartKind::Moved
                } else {
                    GroupStartKind::Reused
                },
            )
        } else {
            (
                self.table
                    .insert_new_group(insert_index, parent_anchor, key),
                GroupStartKind::Inserted,
            )
        };

        let group_index = self.table.open_group_frame(self.state, anchor, kind);
        let scope_id = self.table.groups[group_index].scope_id;

        StartScopedGroup {
            group: GroupId::new(group_index, self.table.groups[group_index].generation),
            anchor,
            scope_id,
            kind,
        }
    }

    pub(crate) fn set_group_scope(&mut self, group: GroupId, scope_id: ScopeId) {
        let group_index = group.index();
        let record = self
            .table
            .groups
            .get_mut(group_index)
            .expect("group handle index missing");
        assert_eq!(
            record.generation,
            group.generation(),
            "group handle generation mismatch"
        );
        if let Some(previous) = record.scope_id.replace(scope_id) {
            self.table.scope_anchor_to_group.remove(&previous);
        }
        self.table
            .scope_anchor_to_group
            .insert(scope_id, record.anchor);
    }

    pub(crate) fn finish_group_body(&mut self) -> FinishGroupResult {
        self.table
            .finish_group_body_internal(self.lifecycle, self.state)
    }

    pub(crate) fn end_group(&mut self) {
        let frame = self
            .state
            .group_stack
            .pop()
            .expect("unbalanced group stack");
        let group_index = self.table.current_group_index(frame.group_anchor);
        let subtree_end = group_index + self.table.groups[group_index].subtree_len as usize;
        if let Some(parent) = self.state.group_stack.last_mut() {
            parent.next_child_index = subtree_end;
        } else {
            self.state.root.next_child_index = subtree_end;
        }
    }

    pub(crate) fn end_recompose(&mut self) {
        self.end_group();
    }

    pub(crate) fn use_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> ValueSlotId {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("value slots require an active group");
        let group_anchor = frame.group_anchor;
        let group_index = self.table.current_group_index(group_anchor);
        let payload_len = self.table.groups[group_index].payloads.len();

        let generation = if frame.payload_cursor < payload_len {
            let record = &mut self.table.groups[group_index].payloads[frame.payload_cursor];
            if record.value.is::<T>() {
                record.generation
            } else {
                let old_value = mem::replace(&mut record.value, Box::new(init()));
                self.lifecycle.queue_drop(DeferredDrop::Value(old_value));
                record.type_name = std::any::type_name::<T>();
                record.inline_payload_bytes = mem::size_of::<T>();
                record.generation
            }
        } else {
            let anchor = self.table.allocate_payload_anchor();
            let generation = 1;
            self.table.insert_payload_record(
                group_anchor,
                frame.payload_cursor,
                PayloadRecord {
                    owner: group_anchor,
                    anchor,
                    generation,
                    type_name: std::any::type_name::<T>(),
                    inline_payload_bytes: mem::size_of::<T>(),
                    value: Box::new(init()),
                },
            );
            generation
        };

        let slot = ValueSlotId::new(
            GroupId::new(group_index, self.table.groups[group_index].generation),
            frame.payload_cursor,
            generation,
        );
        frame.payload_cursor += 1;
        slot
    }

    pub(crate) fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let slot = self.use_value_slot(|| Owned::new(init()));
        self.table.read_value::<Owned<T>>(slot).clone()
    }

    pub(crate) fn record_node(&mut self, id: NodeId, generation: u32) {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("node records require an active group");
        let group_anchor = frame.group_anchor;
        let group_index = self.table.current_group_index(group_anchor);
        let group = &mut self.table.groups[group_index];

        if frame.node_cursor < group.nodes.len() {
            group.nodes[frame.node_cursor] = NodeRecord {
                owner: group_anchor,
                id,
                generation,
            };
        } else {
            group.nodes.insert(
                frame.node_cursor,
                NodeRecord {
                    owner: group_anchor,
                    id,
                    generation,
                },
            );
            self.table.adjust_ancestor_node_counts(group_anchor, 1);
        }

        frame.node_cursor += 1;
    }

    pub(crate) fn current_node_record(&self) -> Option<(NodeId, u32)> {
        let frame = self.state.group_stack.last()?;
        let group = self.table.current_group(frame.group_anchor);
        if frame.node_cursor >= group.nodes.len() {
            return None;
        }
        let node = group.nodes.get(frame.node_cursor)?;
        Some((node.id, node.generation))
    }

    pub(crate) fn consume_reused_node(&mut self) {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("consume_reused_node requires an active group");
        frame.node_cursor += 1;
    }

    pub(crate) fn skip_current_group(&mut self) {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("skip_current_group requires an active group");
        frame.old_cursor = frame.old_children.len();
        frame.payload_cursor = frame.old_payload_len;
        frame.node_cursor = frame.old_node_len;
    }

    pub(crate) fn nodes_in_current_group(&self) -> Vec<NodeId> {
        let frame = self
            .state
            .group_stack
            .last()
            .expect("nodes_in_current_group requires an active group");
        let group_index = self.table.current_group_index(frame.group_anchor);
        let subtree_end = group_index + self.table.groups[group_index].subtree_len as usize;
        self.table.groups[group_index..subtree_end]
            .iter()
            .flat_map(|group| group.nodes.iter())
            .map(|node| node.id)
            .collect()
    }

    pub(crate) fn finalize_pass(&mut self, applier: &mut dyn Applier) {
        while !self.state.group_stack.is_empty() {
            let result = self.finish_group_body();
            for subtree in result.detached_children {
                dispose_detached_subtree_now(applier, &subtree);
                self.lifecycle.queue_subtree_disposal(subtree);
            }
            for node_id in result.direct_nodes {
                dispose_detached_node_now(applier, node_id);
            }
            self.end_group();
        }

        let root_detached = self.table.root_finish_result(self.state);
        self.state.note_detached_subtrees(&root_detached);
        for subtree in root_detached {
            dispose_detached_subtree_now(applier, &subtree);
            self.lifecycle.queue_subtree_disposal(subtree);
        }
    }
}

fn dispose_detached_node_now(applier: &mut dyn Applier, node_id: NodeId) {
    let parent_id = applier.get_mut(node_id).ok().and_then(|node| node.parent());
    if let Some(parent_id) = parent_id {
        let _ = remove_child_and_cleanup_now(applier, parent_id, node_id);
        return;
    }
    if let Ok(node) = applier.get_mut(node_id) {
        node.on_removed_from_parent();
        node.unmount();
    }
    let _ = applier.remove(node_id);
}

fn dispose_detached_subtree_now(applier: &mut dyn Applier, subtree: &DetachedSubtree) {
    let nodes = subtree.node_ids();
    let node_set = nodes
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let roots = nodes
        .into_iter()
        .filter(|id| {
            let parent = applier.get_mut(*id).ok().and_then(|node| node.parent());
            parent.is_none_or(|parent_id| !node_set.contains(&parent_id))
        })
        .collect::<Vec<_>>();

    for root in roots {
        dispose_detached_node_now(applier, root);
    }
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub(crate) fn begin_group_for_test(
    table: &mut SlotTable,
    state: &mut SlotWriteSessionState,
    key: Key,
) -> GroupId {
    let mut lifecycle = SlotLifecycleCoordinator::default();
    let mut session = table.write_session(&mut lifecycle, state, SlotPassMode::Compose);
    let group_key = session.preview_group_key(GroupKeySeed::unkeyed(key));
    let started = session.begin_scoped_group(group_key, None);
    started.group
}

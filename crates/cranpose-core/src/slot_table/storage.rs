use super::{
    anchor_map::AnchorMap,
    lifecycle_queue::{OrphanedNodeIds, PendingDrops},
    NodeSlotState, OrphanedNode, SlotTableDebugStats, SlotValueTypeDebugStat,
};
use crate::{runtime, AnchorId, Key, NodeId, RecomposeScope};
use std::any::Any;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryKind {
    Unused,
    Group,
    Value,
    Node,
    HiddenGroup,
    HiddenValue,
    HiddenNode,
}

impl EntryKind {
    pub(crate) fn is_group(self) -> bool {
        matches!(self, Self::Group | Self::HiddenGroup)
    }

    pub(crate) fn is_hidden(self) -> bool {
        matches!(
            self,
            Self::HiddenGroup | Self::HiddenValue | Self::HiddenNode
        )
    }

    pub(crate) fn restore(self) -> Self {
        match self {
            Self::HiddenGroup => Self::Group,
            Self::HiddenValue => Self::Value,
            Self::HiddenNode => Self::Node,
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EntryDescriptor {
    pub(crate) kind: EntryKind,
    pub(crate) anchor: AnchorId,
    pub(crate) payload: u32,
}

#[derive(Clone)]
pub(crate) struct GroupSnapshot {
    pub(crate) key: Key,
    pub(crate) len: u32,
    pub(crate) boundary_key: Key,
    pub(crate) has_hidden_children: bool,
    pub(crate) scope: Option<RecomposeScope>,
}

enum DeferredDrop {
    Group(Option<RecomposeScope>),
    Value(Box<dyn Any>),
}

impl DeferredDrop {
    fn dispose(self) {
        match self {
            Self::Group(scope) => drop(scope),
            Self::Value(value) => drop(value),
        }
    }
}

struct GroupRecord {
    key: Key,
    len: u32,
    boundary_key: Key,
    has_hidden_children: bool,
    parent_anchor: AnchorId,
    scope: Option<RecomposeScope>,
}

struct ValueRecord {
    type_name: &'static str,
    inline_payload_bytes: usize,
    value: Box<dyn Any>,
}

struct NodeRecord {
    id: NodeId,
    generation: u32,
}

#[derive(Default)]
struct GroupArena {
    keys: Vec<Key>,
    lens: Vec<u32>,
    boundary_keys: Vec<Key>,
    has_hidden_children: Vec<bool>,
    parent_anchors: Vec<AnchorId>,
    scopes: Vec<Option<RecomposeScope>>,
    free: Vec<u32>,
}

impl GroupArena {
    fn push_record(&mut self, record: GroupRecord) -> u32 {
        let id = self.keys.len() as u32;
        self.keys.push(record.key);
        self.lens.push(record.len);
        self.boundary_keys.push(record.boundary_key);
        self.has_hidden_children.push(record.has_hidden_children);
        self.parent_anchors.push(record.parent_anchor);
        self.scopes.push(record.scope);
        id
    }

    fn alloc(
        &mut self,
        key: Key,
        len: u32,
        boundary_key: Key,
        parent_anchor: AnchorId,
        scope: Option<RecomposeScope>,
    ) -> u32 {
        if let Some(id) = self.free.pop() {
            let idx = id as usize;
            self.keys[idx] = key;
            self.lens[idx] = len;
            self.boundary_keys[idx] = boundary_key;
            self.has_hidden_children[idx] = false;
            self.parent_anchors[idx] = parent_anchor;
            self.scopes[idx] = scope;
            id
        } else {
            let id = self.keys.len() as u32;
            self.keys.push(key);
            self.lens.push(len);
            self.boundary_keys.push(boundary_key);
            self.has_hidden_children.push(false);
            self.parent_anchors.push(parent_anchor);
            self.scopes.push(scope);
            id
        }
    }

    fn snapshot(&self, id: u32) -> GroupSnapshot {
        let idx = id as usize;
        GroupSnapshot {
            key: self.keys[idx],
            len: self.lens[idx],
            boundary_key: self.boundary_keys[idx],
            has_hidden_children: self.has_hidden_children[idx],
            scope: self.scopes[idx].clone(),
        }
    }

    fn key(&self, id: u32) -> Key {
        self.keys[id as usize]
    }

    fn len(&self, id: u32) -> usize {
        self.lens[id as usize] as usize
    }

    fn set_len(&mut self, id: u32, len: usize) {
        self.lens[id as usize] = len as u32;
    }

    fn set_boundary_key(&mut self, id: u32, boundary_key: Key) {
        self.boundary_keys[id as usize] = boundary_key;
    }

    fn set_has_hidden_children(&mut self, id: u32, has_hidden_children: bool) {
        self.has_hidden_children[id as usize] = has_hidden_children;
    }

    fn parent_anchor(&self, id: u32) -> AnchorId {
        self.parent_anchors[id as usize]
    }

    fn scope(&self, id: u32) -> Option<&RecomposeScope> {
        self.scopes[id as usize].as_ref()
    }

    fn set_scope(&mut self, id: u32, scope: Option<RecomposeScope>) {
        self.scopes[id as usize] = scope;
    }

    fn clear_scope(&mut self, id: u32) {
        let idx = id as usize;
        if let Some(scope) = self.scopes[idx].take() {
            scope.deactivate();
        }
    }

    fn deactivate_scope(&self, id: u32) {
        if let Some(scope) = self.scopes[id as usize].as_ref() {
            scope.deactivate();
        }
    }

    fn take_drop(&mut self, id: u32) -> DeferredDrop {
        let idx = id as usize;
        let scope = self.scopes[idx].take();
        self.keys[idx] = 0;
        self.lens[idx] = 0;
        self.boundary_keys[idx] = 0;
        self.has_hidden_children[idx] = false;
        self.parent_anchors[idx] = AnchorId::INVALID;
        self.free.push(id);
        DeferredDrop::Group(scope)
    }

    fn take_live(&mut self, id: u32) -> GroupRecord {
        let idx = id as usize;
        GroupRecord {
            key: std::mem::take(&mut self.keys[idx]),
            len: std::mem::take(&mut self.lens[idx]),
            boundary_key: std::mem::take(&mut self.boundary_keys[idx]),
            has_hidden_children: std::mem::take(&mut self.has_hidden_children[idx]),
            parent_anchor: std::mem::replace(&mut self.parent_anchors[idx], AnchorId::INVALID),
            scope: self.scopes[idx].take(),
        }
    }

    fn heap_bytes(&self) -> usize {
        self.keys.capacity() * std::mem::size_of::<Key>()
            + self.lens.capacity() * std::mem::size_of::<u32>()
            + self.boundary_keys.capacity() * std::mem::size_of::<Key>()
            + self.has_hidden_children.capacity() * std::mem::size_of::<bool>()
            + self.parent_anchors.capacity() * std::mem::size_of::<AnchorId>()
            + self.scopes.capacity() * std::mem::size_of::<Option<RecomposeScope>>()
            + self.free.capacity() * std::mem::size_of::<u32>()
    }
}

#[derive(Default)]
struct ValueArena {
    type_names: Vec<&'static str>,
    inline_payload_bytes: Vec<usize>,
    values: Vec<Option<Box<dyn Any>>>,
    free: Vec<u32>,
}

impl ValueArena {
    fn push_record(&mut self, record: ValueRecord) -> u32 {
        let id = self.values.len() as u32;
        self.type_names.push(record.type_name);
        self.inline_payload_bytes.push(record.inline_payload_bytes);
        self.values.push(Some(record.value));
        id
    }

    fn alloc<T: 'static>(&mut self, value: T) -> u32 {
        let type_name = std::any::type_name::<T>();
        let payload_bytes = std::mem::size_of::<T>();
        let boxed: Box<dyn Any> = Box::new(value);
        if let Some(id) = self.free.pop() {
            let idx = id as usize;
            self.type_names[idx] = type_name;
            self.inline_payload_bytes[idx] = payload_bytes;
            self.values[idx] = Some(boxed);
            id
        } else {
            let id = self.values.len() as u32;
            self.type_names.push(type_name);
            self.inline_payload_bytes.push(payload_bytes);
            self.values.push(Some(boxed));
            id
        }
    }

    fn is_type<T: 'static>(&self, id: u32) -> bool {
        self.values[id as usize]
            .as_deref()
            .is_some_and(|value| value.is::<T>())
    }

    fn read<T: 'static>(&self, id: u32) -> &T {
        self.values[id as usize]
            .as_deref()
            .and_then(|value| value.downcast_ref::<T>())
            .expect("slot value type mismatch")
    }

    fn read_mut<T: 'static>(&mut self, id: u32) -> &mut T {
        self.values[id as usize]
            .as_deref_mut()
            .and_then(|value| value.downcast_mut::<T>())
            .expect("slot value type mismatch")
    }

    fn write<T: 'static>(&mut self, id: u32, value: T) {
        let idx = id as usize;
        self.type_names[idx] = std::any::type_name::<T>();
        self.inline_payload_bytes[idx] = std::mem::size_of::<T>();
        self.values[idx] = Some(Box::new(value));
    }

    fn type_debug_stat(&self, id: u32) -> SlotValueTypeDebugStat {
        let idx = id as usize;
        SlotValueTypeDebugStat {
            type_name: self.type_names[idx],
            count: 1,
            inline_payload_bytes: self.inline_payload_bytes[idx],
        }
    }

    fn take_drop(&mut self, id: u32) -> DeferredDrop {
        let idx = id as usize;
        let value = self.values[idx].take().expect("live value payload");
        self.type_names[idx] = "";
        self.inline_payload_bytes[idx] = 0;
        self.free.push(id);
        DeferredDrop::Value(value)
    }

    fn take_live(&mut self, id: u32) -> ValueRecord {
        let idx = id as usize;
        ValueRecord {
            type_name: std::mem::take(&mut self.type_names[idx]),
            inline_payload_bytes: std::mem::take(&mut self.inline_payload_bytes[idx]),
            value: self.values[idx].take().expect("live value payload"),
        }
    }

    fn heap_bytes(&self) -> usize {
        self.type_names.capacity() * std::mem::size_of::<&'static str>()
            + self.inline_payload_bytes.capacity() * std::mem::size_of::<usize>()
            + self.values.capacity() * std::mem::size_of::<Option<Box<dyn Any>>>()
            + self.free.capacity() * std::mem::size_of::<u32>()
    }
}

#[derive(Default)]
struct NodeArena {
    ids: Vec<NodeId>,
    generations: Vec<u32>,
    live: Vec<bool>,
    free: Vec<u32>,
}

impl NodeArena {
    fn push_record(&mut self, record: NodeRecord) -> u32 {
        let node = self.ids.len() as u32;
        self.ids.push(record.id);
        self.generations.push(record.generation);
        self.live.push(true);
        node
    }

    fn alloc(&mut self, id: NodeId, generation: u32) -> u32 {
        if let Some(node) = self.free.pop() {
            let idx = node as usize;
            self.ids[idx] = id;
            self.generations[idx] = generation;
            self.live[idx] = true;
            node
        } else {
            let node = self.ids.len() as u32;
            self.ids.push(id);
            self.generations.push(generation);
            self.live.push(true);
            node
        }
    }

    fn get(&self, node: u32) -> (NodeId, u32) {
        let idx = node as usize;
        (self.ids[idx], self.generations[idx])
    }

    fn free(&mut self, node: u32) {
        let idx = node as usize;
        self.ids[idx] = 0;
        self.generations[idx] = 0;
        self.live[idx] = false;
        self.free.push(node);
    }

    fn take_live(&mut self, node: u32) -> NodeRecord {
        let idx = node as usize;
        let record = NodeRecord {
            id: std::mem::take(&mut self.ids[idx]),
            generation: std::mem::take(&mut self.generations[idx]),
        };
        self.live[idx] = false;
        record
    }

    fn heap_bytes(&self) -> usize {
        self.ids.capacity() * std::mem::size_of::<NodeId>()
            + self.generations.capacity() * std::mem::size_of::<u32>()
            + self.live.capacity() * std::mem::size_of::<bool>()
            + self.free.capacity() * std::mem::size_of::<u32>()
    }
}

struct EntryBuffer {
    kinds: Vec<EntryKind>,
    anchors: Vec<AnchorId>,
    payloads: Vec<u32>,
    len: usize,
    gap_start: usize,
    gap_len: usize,
}

impl EntryBuffer {
    fn new() -> Self {
        Self {
            kinds: Vec::new(),
            anchors: Vec::new(),
            payloads: Vec::new(),
            len: 0,
            gap_start: 0,
            gap_len: 0,
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn capacity(&self) -> usize {
        self.kinds.len()
    }

    fn gap_len(&self) -> usize {
        self.gap_len
    }

    fn physical_index(&self, logical_index: usize) -> usize {
        debug_assert!(logical_index < self.len, "logical index out of bounds");
        if logical_index < self.gap_start {
            logical_index
        } else {
            logical_index + self.gap_len
        }
    }

    fn entry(&self, logical_index: usize) -> Option<EntryDescriptor> {
        (logical_index < self.len).then(|| {
            let physical = self.physical_index(logical_index);
            EntryDescriptor {
                kind: self.kinds[physical],
                anchor: self.anchors[physical],
                payload: self.payloads[physical],
            }
        })
    }

    fn set_entry(&mut self, logical_index: usize, entry: EntryDescriptor) {
        let physical = self.physical_index(logical_index);
        self.kinds[physical] = entry.kind;
        self.anchors[physical] = entry.anchor;
        self.payloads[physical] = entry.payload;
    }

    fn set_kind(&mut self, logical_index: usize, kind: EntryKind) {
        let physical = self.physical_index(logical_index);
        self.kinds[physical] = kind;
    }

    fn grow(&mut self, target_capacity: usize) {
        if target_capacity <= self.capacity() {
            return;
        }
        let additional = target_capacity - self.capacity();
        self.kinds.splice(
            self.gap_start..self.gap_start,
            std::iter::repeat_n(EntryKind::Unused, additional),
        );
        self.anchors.splice(
            self.gap_start..self.gap_start,
            std::iter::repeat_n(AnchorId::INVALID, additional),
        );
        self.payloads.splice(
            self.gap_start..self.gap_start,
            std::iter::repeat_n(0, additional),
        );
        self.gap_len += additional;
    }

    fn move_gap_to(&mut self, index: usize, anchor_map: &mut AnchorMap) {
        if index == self.gap_start {
            return;
        }

        if index < self.gap_start {
            for logical in index..self.gap_start {
                let anchor = self.anchors[logical];
                anchor_map.update_for_gap_move(anchor, logical, index, self.len);
            }
            let end = self.gap_start + self.gap_len;
            self.kinds[index..end].rotate_right(self.gap_len);
            self.anchors[index..end].rotate_right(self.gap_len);
            self.payloads[index..end].rotate_right(self.gap_len);
        } else {
            for logical in self.gap_start..index {
                let physical = logical + self.gap_len;
                let anchor = self.anchors[physical];
                anchor_map.update_for_gap_move(anchor, logical, index, self.len);
            }
            let end = index + self.gap_len;
            self.kinds[self.gap_start..end].rotate_left(self.gap_len);
            self.anchors[self.gap_start..end].rotate_left(self.gap_len);
            self.payloads[self.gap_start..end].rotate_left(self.gap_len);
        }

        self.gap_start = index;
    }

    fn insert_entry(&mut self, index: usize, entry: EntryDescriptor, anchor_map: &mut AnchorMap) {
        debug_assert!(index <= self.len, "insert index out of bounds");
        self.move_gap_to(index, anchor_map);
        debug_assert!(self.gap_len > 0, "insert requires capacity gap");

        let physical = self.gap_start;
        self.kinds[physical] = entry.kind;
        self.anchors[physical] = entry.anchor;
        self.payloads[physical] = entry.payload;
        self.gap_start += 1;
        self.gap_len -= 1;
        self.len += 1;

        if entry.anchor.is_valid() {
            anchor_map.register_anchor(entry.anchor, index, self.gap_start, self.len);
        }
    }

    fn remove_range(
        &mut self,
        start: usize,
        len: usize,
        anchor_map: &mut AnchorMap,
    ) -> Vec<EntryDescriptor> {
        if len == 0 {
            return Vec::new();
        }

        self.move_gap_to(start, anchor_map);
        let mut removed = Vec::with_capacity(len);
        let physical_start = self.gap_start + self.gap_len;
        for offset in 0..len {
            let physical = physical_start + offset;
            removed.push(EntryDescriptor {
                kind: self.kinds[physical],
                anchor: self.anchors[physical],
                payload: self.payloads[physical],
            });
            self.kinds[physical] = EntryKind::Unused;
            self.anchors[physical] = AnchorId::INVALID;
            self.payloads[physical] = 0;
        }
        self.gap_len += len;
        self.len -= len;
        removed
    }

    fn insert_range(
        &mut self,
        index: usize,
        entries: &[EntryDescriptor],
        anchor_map: &mut AnchorMap,
    ) {
        if entries.is_empty() {
            return;
        }

        self.move_gap_to(index, anchor_map);
        debug_assert!(
            self.gap_len >= entries.len(),
            "insert range requires capacity gap"
        );

        for (offset, entry) in entries.iter().enumerate() {
            let physical = self.gap_start;
            self.kinds[physical] = entry.kind;
            self.anchors[physical] = entry.anchor;
            self.payloads[physical] = entry.payload;
            self.gap_start += 1;
            self.gap_len -= 1;
            self.len += 1;
            if entry.anchor.is_valid() {
                anchor_map.register_anchor(entry.anchor, index + offset, self.gap_start, self.len);
            }
        }
    }

    fn reset_from_live_entries(&mut self, entries: &[EntryDescriptor]) {
        self.kinds = Vec::with_capacity(entries.len());
        self.anchors = Vec::with_capacity(entries.len());
        self.payloads = Vec::with_capacity(entries.len());
        for entry in entries {
            self.kinds.push(entry.kind);
            self.anchors.push(entry.anchor);
            self.payloads.push(entry.payload);
        }
        self.len = entries.len();
        self.gap_start = entries.len();
        self.gap_len = 0;
    }

    fn heap_bytes(&self) -> usize {
        self.kinds.capacity() * std::mem::size_of::<EntryKind>()
            + self.anchors.capacity() * std::mem::size_of::<AnchorId>()
            + self.payloads.capacity() * std::mem::size_of::<u32>()
    }
}

pub(crate) struct SlotStorage {
    buffer: EntryBuffer,
    groups: GroupArena,
    values: ValueArena,
    nodes: NodeArena,
    pending_drops: PendingDrops<DeferredDrop>,
    anchor_map: AnchorMap,
    orphaned_node_ids: OrphanedNodeIds,
    pub(crate) needs_compact: bool,
}

impl SlotStorage {
    const MIN_RETAINED_PENDING_DROPS_CAPACITY: usize = 4;

    pub(crate) fn new() -> Self {
        Self {
            buffer: EntryBuffer::new(),
            groups: GroupArena::default(),
            values: ValueArena::default(),
            nodes: NodeArena::default(),
            pending_drops: PendingDrops::default(),
            anchor_map: AnchorMap::default(),
            orphaned_node_ids: OrphanedNodeIds::default(),
            needs_compact: false,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.buffer.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.buffer.capacity()
    }

    pub(crate) fn gap_len(&self) -> usize {
        self.buffer.gap_len()
    }

    pub(crate) fn grow(&mut self, target_capacity: usize) {
        self.buffer.grow(target_capacity);
    }

    pub(crate) fn ensure_gap_at(&mut self, index: usize) {
        self.buffer.move_gap_to(index, &mut self.anchor_map);
    }

    pub(crate) fn take_anchors_dirty(&mut self) -> bool {
        self.anchor_map.take_dirty()
    }

    pub(crate) fn rebuild_anchor_positions(&mut self) {
        let live_anchors = (0..self.buffer.len()).filter_map(|index| {
            let entry = self.buffer.entry(index)?;
            entry.anchor.is_valid().then_some((entry.anchor, index))
        });
        self.anchor_map
            .rebuild_positions(live_anchors, self.buffer.gap_start, self.buffer.len());
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.buffer.heap_bytes()
            + self.groups.heap_bytes()
            + self.values.heap_bytes()
            + self.nodes.heap_bytes()
            + self.pending_drops.capacity() * std::mem::size_of::<DeferredDrop>()
            + self.anchor_map.debug_heap_bytes()
            + self.orphaned_node_ids.capacity() * std::mem::size_of::<OrphanedNode>()
    }

    pub(crate) fn debug_stats(&self) -> SlotTableDebugStats {
        let mut stats = SlotTableDebugStats {
            slots_len: self.buffer.len(),
            slots_cap: self.buffer.capacity(),
            pending_slot_drops_len: self.pending_drops.len(),
            pending_slot_drops_cap: self.pending_drops.capacity(),
            anchors_len: 0,
            anchors_cap: 0,
            gap_metadata_len: 0,
            gap_metadata_cap: 0,
            free_anchor_ids_len: 0,
            free_anchor_ids_cap: 0,
            group_stack_len: 0,
            group_stack_cap: 0,
            orphaned_node_ids_len: self.orphaned_node_ids.len(),
            orphaned_node_ids_cap: self.orphaned_node_ids.capacity(),
        };
        self.anchor_map.fill_debug_stats(&mut stats);
        stats
    }

    pub(crate) fn entry(&self, index: usize) -> Option<EntryDescriptor> {
        self.buffer.entry(index)
    }

    pub(crate) fn entry_kind(&self, index: usize) -> Option<EntryKind> {
        self.entry(index).map(|entry| entry.kind)
    }

    pub(crate) fn entry_anchor(&self, index: usize) -> AnchorId {
        self.entry(index)
            .map(|entry| entry.anchor)
            .unwrap_or(AnchorId::INVALID)
    }

    pub(crate) fn entry_extent(&self, index: usize) -> usize {
        match self.entry(index).map(|entry| entry.kind) {
            Some(kind) if kind.is_group() => self.group_len_at(index),
            Some(_) => 1,
            None => 0,
        }
    }

    pub(crate) fn group_id_at(&self, index: usize) -> Option<u32> {
        let entry = self.entry(index)?;
        entry.kind.is_group().then_some(entry.payload)
    }

    pub(crate) fn live_group_id_at(&self, index: usize) -> Option<u32> {
        let entry = self.entry(index)?;
        (entry.kind == EntryKind::Group).then_some(entry.payload)
    }

    pub(crate) fn group_snapshot_at(&self, index: usize) -> Option<GroupSnapshot> {
        let group = self.group_id_at(index)?;
        Some(self.groups.snapshot(group))
    }

    pub(crate) fn group_len_at(&self, index: usize) -> usize {
        self.group_id_at(index)
            .map(|group| self.groups.len(group))
            .unwrap_or(0)
    }

    pub(crate) fn group_key_at(&self, index: usize) -> Option<Key> {
        self.group_id_at(index).map(|group| self.groups.key(group))
    }

    pub(crate) fn live_group_scope(&self, index: usize) -> Option<&RecomposeScope> {
        let group = self.live_group_id_at(index)?;
        self.groups.scope(group)
    }

    pub(crate) fn live_group_has_scope(&self, index: usize) -> bool {
        self.live_group_scope(index).is_some()
    }

    pub(crate) fn clear_group_scope(&mut self, index: usize) {
        if let Some(group) = self.group_id_at(index) {
            self.groups.clear_scope(group);
        }
    }

    pub(crate) fn set_group_scope(&mut self, index: usize, scope: Option<RecomposeScope>) {
        if let Some(group) = self.group_id_at(index) {
            self.groups.set_scope(group, scope);
        }
    }

    pub(crate) fn set_group_len(&mut self, index: usize, len: usize) {
        if let Some(group) = self.group_id_at(index) {
            self.groups.set_len(group, len);
        }
    }

    pub(crate) fn set_group_boundary_key(&mut self, index: usize, boundary_key: Key) {
        if let Some(group) = self.group_id_at(index) {
            self.groups.set_boundary_key(group, boundary_key);
        }
    }

    pub(crate) fn set_group_has_hidden_children(
        &mut self,
        index: usize,
        has_hidden_children: bool,
    ) {
        if let Some(group) = self.group_id_at(index) {
            self.groups
                .set_has_hidden_children(group, has_hidden_children);
        }
    }

    pub(crate) fn group_parent_anchor_at(&self, index: usize) -> Option<AnchorId> {
        self.group_id_at(index)
            .map(|group| self.groups.parent_anchor(group))
    }

    pub(crate) fn alloc_group(
        &mut self,
        key: Key,
        boundary_key: Key,
        parent_anchor: AnchorId,
        scope: Option<RecomposeScope>,
    ) -> u32 {
        self.groups
            .alloc(key, 0, boundary_key, parent_anchor, scope)
    }

    pub(crate) fn insert_group(
        &mut self,
        index: usize,
        anchor: AnchorId,
        group: u32,
        hidden: bool,
    ) {
        self.buffer.insert_entry(
            index,
            EntryDescriptor {
                kind: if hidden {
                    EntryKind::HiddenGroup
                } else {
                    EntryKind::Group
                },
                anchor,
                payload: group,
            },
            &mut self.anchor_map,
        );
    }

    pub(crate) fn alloc_value<T: 'static>(&mut self, value: T) -> u32 {
        self.values.alloc(value)
    }

    pub(crate) fn insert_value(
        &mut self,
        index: usize,
        anchor: AnchorId,
        value: u32,
        hidden: bool,
    ) {
        self.buffer.insert_entry(
            index,
            EntryDescriptor {
                kind: if hidden {
                    EntryKind::HiddenValue
                } else {
                    EntryKind::Value
                },
                anchor,
                payload: value,
            },
            &mut self.anchor_map,
        );
    }

    pub(crate) fn overwrite_value(
        &mut self,
        index: usize,
        anchor: AnchorId,
        value: u32,
        hidden: bool,
    ) {
        self.drop_entry(index);
        self.buffer.set_entry(
            index,
            EntryDescriptor {
                kind: if hidden {
                    EntryKind::HiddenValue
                } else {
                    EntryKind::Value
                },
                anchor,
                payload: value,
            },
        );
        self.anchor_map
            .register_anchor(anchor, index, self.buffer.gap_start, self.buffer.len());
    }

    pub(crate) fn value_matches_type<T: 'static>(&self, index: usize) -> bool {
        let Some(entry) = self.entry(index) else {
            return false;
        };
        matches!(entry.kind, EntryKind::Value | EntryKind::HiddenValue)
            && self.values.is_type::<T>(entry.payload)
    }

    pub(crate) fn read_value<T: 'static>(&self, index: usize) -> &T {
        let entry = self.entry(index).expect("slot index out of bounds");
        debug_assert!(
            matches!(entry.kind, EntryKind::Value | EntryKind::HiddenValue),
            "slot is not a value"
        );
        self.values.read(entry.payload)
    }

    pub(crate) fn read_value_mut<T: 'static>(&mut self, index: usize) -> &mut T {
        let entry = self.entry(index).expect("slot index out of bounds");
        debug_assert!(
            matches!(entry.kind, EntryKind::Value | EntryKind::HiddenValue),
            "slot is not a value"
        );
        self.values.read_mut(entry.payload)
    }

    pub(crate) fn write_value<T: 'static>(&mut self, index: usize, value: T) {
        let entry = self.entry(index).expect("slot index out of bounds");
        debug_assert!(
            matches!(entry.kind, EntryKind::Value | EntryKind::HiddenValue),
            "slot is not a value"
        );
        self.values.write(entry.payload, value);
    }

    pub(crate) fn alloc_node(&mut self, id: NodeId, generation: u32) -> u32 {
        self.nodes.alloc(id, generation)
    }

    pub(crate) fn insert_node(&mut self, index: usize, anchor: AnchorId, node: u32, hidden: bool) {
        self.buffer.insert_entry(
            index,
            EntryDescriptor {
                kind: if hidden {
                    EntryKind::HiddenNode
                } else {
                    EntryKind::Node
                },
                anchor,
                payload: node,
            },
            &mut self.anchor_map,
        );
    }

    pub(crate) fn overwrite_node(
        &mut self,
        index: usize,
        anchor: AnchorId,
        node: u32,
        hidden: bool,
    ) {
        self.drop_entry(index);
        self.buffer.set_entry(
            index,
            EntryDescriptor {
                kind: if hidden {
                    EntryKind::HiddenNode
                } else {
                    EntryKind::Node
                },
                anchor,
                payload: node,
            },
        );
        self.anchor_map
            .register_anchor(anchor, index, self.buffer.gap_start, self.buffer.len());
    }

    pub(crate) fn node_at(&self, index: usize) -> Option<(EntryKind, NodeId, u32)> {
        let entry = self.entry(index)?;
        matches!(entry.kind, EntryKind::Node | EntryKind::HiddenNode).then(|| {
            let (id, generation) = self.nodes.get(entry.payload);
            (entry.kind, id, generation)
        })
    }

    pub(crate) fn hide_entry(&mut self, index: usize) {
        let Some(entry) = self.entry(index) else {
            return;
        };

        match entry.kind {
            EntryKind::Group => {
                self.groups.deactivate_scope(entry.payload);
                self.buffer.set_kind(index, EntryKind::HiddenGroup);
            }
            EntryKind::Value => {
                self.buffer.set_kind(index, EntryKind::HiddenValue);
            }
            EntryKind::Node => {
                let (id, generation) = self.nodes.get(entry.payload);
                self.orphaned_node_ids
                    .push(OrphanedNode::new(id, generation, entry.anchor));
                self.buffer.set_kind(index, EntryKind::HiddenNode);
            }
            EntryKind::HiddenGroup
            | EntryKind::HiddenValue
            | EntryKind::HiddenNode
            | EntryKind::Unused => {}
        }
    }

    pub(crate) fn restore_hidden_entry(&mut self, index: usize) {
        if let Some(entry) = self.entry(index) {
            self.buffer.set_kind(index, entry.kind.restore());
        }
    }

    pub(crate) fn drop_entry(&mut self, index: usize) {
        let Some(entry) = self.entry(index) else {
            return;
        };

        match entry.kind {
            EntryKind::Group | EntryKind::HiddenGroup => {
                self.pending_drops
                    .push(self.groups.take_drop(entry.payload));
            }
            EntryKind::Value | EntryKind::HiddenValue => {
                self.pending_drops
                    .push(self.values.take_drop(entry.payload));
            }
            EntryKind::Node | EntryKind::HiddenNode => {
                self.nodes.free(entry.payload);
            }
            EntryKind::Unused => {}
        }

        if entry.anchor.is_valid() {
            self.anchor_map.free_anchor(entry.anchor);
        }
        self.buffer.set_entry(
            index,
            EntryDescriptor {
                kind: EntryKind::Unused,
                anchor: AnchorId::INVALID,
                payload: 0,
            },
        );
    }

    pub(crate) fn flush_pending_drops(&mut self) {
        self.pending_drops.clear_and_drop_reverse();
        let retained = self
            .pending_drops
            .len()
            .max(Self::MIN_RETAINED_PENDING_DROPS_CAPACITY);
        self.pending_drops.trim_retained_capacity(retained);
    }

    pub(crate) fn drain_orphaned_node_ids_with(&mut self, visitor: impl FnMut(OrphanedNode)) {
        self.orphaned_node_ids.drain_forward(visitor);
    }

    pub(crate) fn drain_orphaned_node_ids(&mut self) -> Vec<OrphanedNode> {
        let mut orphaned = Vec::with_capacity(self.orphaned_node_ids.len());
        self.drain_orphaned_node_ids_with(|node| orphaned.push(node));
        orphaned
    }

    pub(crate) fn requeue_orphaned_node(&mut self, orphaned: OrphanedNode) {
        self.orphaned_node_ids.push(orphaned);
    }

    pub(crate) fn orphaned_node_state(&self, orphaned: OrphanedNode) -> NodeSlotState {
        let Some(index) = self.resolve_anchor(orphaned.anchor) else {
            return NodeSlotState::Missing;
        };

        match self.node_at(index) {
            Some((EntryKind::Node, id, generation))
                if id == orphaned.id && generation == orphaned.generation =>
            {
                NodeSlotState::Active
            }
            Some((EntryKind::HiddenNode, id, generation))
                if id == orphaned.id && generation == orphaned.generation =>
            {
                NodeSlotState::PreservedGap
            }
            _ => NodeSlotState::Missing,
        }
    }

    pub(crate) fn resolve_anchor(&self, anchor: AnchorId) -> Option<usize> {
        self.anchor_map
            .resolve_anchor(anchor, self.buffer.gap_start, self.buffer.len())
    }

    pub(crate) fn allocate_anchor(&mut self) -> AnchorId {
        self.anchor_map.allocate_anchor()
    }

    pub(crate) fn remove_entry_range(&mut self, start: usize, len: usize) -> Vec<EntryDescriptor> {
        self.buffer.remove_range(start, len, &mut self.anchor_map)
    }

    pub(crate) fn insert_entry_range(&mut self, index: usize, entries: &[EntryDescriptor]) {
        self.buffer
            .insert_range(index, entries, &mut self.anchor_map);
    }

    pub(crate) fn compact(
        &mut self,
        eager_compact_slot_len: usize,
        fractional_compact_gap_threshold: usize,
        fractional_compact_ratio_divisor: usize,
    ) {
        if !self.needs_compact {
            return;
        }

        let old_len = self.buffer.len();
        if old_len == 0 {
            self.needs_compact = false;
            return;
        }

        let hidden_count = (0..old_len)
            .filter(|&index| self.entry_kind(index).is_some_and(EntryKind::is_hidden))
            .count();
        if hidden_count == 0 {
            self.needs_compact = false;
            return;
        }

        let new_len = old_len - hidden_count;
        if !Self::should_compact(
            old_len,
            hidden_count,
            new_len,
            eager_compact_slot_len,
            fractional_compact_gap_threshold,
            fractional_compact_ratio_divisor,
        ) {
            return;
        }

        self.needs_compact = false;

        let mut survivors = Vec::with_capacity(new_len);
        let mut removed = Vec::with_capacity(hidden_count);
        let mut stack = Vec::<CompactionFrame>::new();
        let mut kept = 0usize;
        let mut index = 0usize;

        while index < old_len {
            while stack.last().is_some_and(|frame| frame.end <= index) {
                let frame = stack.pop().expect("compaction frame");
                self.groups
                    .set_len(frame.group, kept.saturating_sub(frame.kept_before));
                self.groups.set_has_hidden_children(frame.group, false);
            }

            let entry = self.entry(index).expect("entry during compaction");
            if entry.kind.is_hidden() {
                if entry.kind == EntryKind::HiddenGroup {
                    let hidden_len = self.group_len_at(index);
                    for hidden_index in index..index + hidden_len {
                        let hidden_entry = self.entry(hidden_index).expect("hidden entry");
                        self.collect_removed_entry(hidden_entry, &mut removed);
                    }
                    index += hidden_len;
                } else {
                    self.collect_removed_entry(entry, &mut removed);
                    index += 1;
                }
                continue;
            }

            if entry.kind == EntryKind::Group {
                stack.push(CompactionFrame {
                    group: entry.payload,
                    end: index + self.group_len_at(index),
                    kept_before: kept,
                });
            }

            survivors.push(entry);
            kept += 1;
            index += 1;
        }

        while let Some(frame) = stack.pop() {
            self.groups
                .set_len(frame.group, kept.saturating_sub(frame.kept_before));
            self.groups.set_has_hidden_children(frame.group, false);
        }

        self.rebuild_live_arenas(&mut survivors);
        self.buffer.reset_from_live_entries(&survivors);
        self.rebuild_anchor_positions();

        let _teardown = runtime::enter_state_teardown_scope();
        while let Some(deferred_drop) = removed.pop() {
            deferred_drop.dispose();
        }

        self.orphaned_node_ids
            .trim_retained_capacity(OrphanedNodeIds::INITIAL_CAPACITY);
    }

    pub(crate) fn debug_value_type_counts(&self, limit: usize) -> Vec<SlotValueTypeDebugStat> {
        let mut counts =
            crate::collections::map::HashMap::<&'static str, (usize, usize)>::default();
        for index in 0..self.buffer.len() {
            let Some(entry) = self.entry(index) else {
                continue;
            };
            match entry.kind {
                EntryKind::Group | EntryKind::HiddenGroup => {
                    if self.groups.scope(entry.payload).is_some() {
                        let name = std::any::type_name::<RecomposeScope>();
                        let bytes = std::mem::size_of::<RecomposeScope>();
                        let count = counts.entry(name).or_insert((0, 0));
                        count.0 += 1;
                        count.1 += bytes;
                    }
                }
                EntryKind::Value | EntryKind::HiddenValue => {
                    let stat = self.values.type_debug_stat(entry.payload);
                    let count = counts.entry(stat.type_name).or_insert((0, 0));
                    count.0 += 1;
                    count.1 += stat.inline_payload_bytes;
                }
                EntryKind::Node | EntryKind::HiddenNode | EntryKind::Unused => {}
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
        stats.sort_by_key(|entry| std::cmp::Reverse(entry.count));
        stats.truncate(limit);
        stats
    }

    pub(crate) fn drop_all_reverse(&mut self) {
        let mut drops = Vec::with_capacity(self.buffer.len());
        for index in 0..self.buffer.len() {
            let entry = self.entry(index).expect("entry during drop");
            self.collect_removed_entry(entry, &mut drops);
        }
        self.buffer.reset_from_live_entries(&[]);
        self.groups = GroupArena::default();
        self.values = ValueArena::default();
        self.nodes = NodeArena::default();
        self.anchor_map
            .rebuild_positions(std::iter::empty::<(AnchorId, usize)>(), 0, 0);

        let _teardown = runtime::enter_state_teardown_scope();
        while let Some(deferred_drop) = drops.pop() {
            deferred_drop.dispose();
        }
    }

    fn collect_removed_entry(&mut self, entry: EntryDescriptor, removed: &mut Vec<DeferredDrop>) {
        match entry.kind {
            EntryKind::Group | EntryKind::HiddenGroup => {
                removed.push(self.groups.take_drop(entry.payload));
            }
            EntryKind::Value | EntryKind::HiddenValue => {
                removed.push(self.values.take_drop(entry.payload));
            }
            EntryKind::Node | EntryKind::HiddenNode => {
                self.nodes.free(entry.payload);
            }
            EntryKind::Unused => {}
        }
        if entry.anchor.is_valid() {
            self.anchor_map.free_anchor(entry.anchor);
        }
    }

    fn rebuild_live_arenas(&mut self, survivors: &mut [EntryDescriptor]) {
        let mut groups = std::mem::take(&mut self.groups);
        let mut values = std::mem::take(&mut self.values);
        let mut nodes = std::mem::take(&mut self.nodes);

        let mut rebuilt_groups = GroupArena::default();
        let mut rebuilt_values = ValueArena::default();
        let mut rebuilt_nodes = NodeArena::default();

        for entry in survivors {
            entry.payload = match entry.kind {
                EntryKind::Group => rebuilt_groups.push_record(groups.take_live(entry.payload)),
                EntryKind::Value => rebuilt_values.push_record(values.take_live(entry.payload)),
                EntryKind::Node => rebuilt_nodes.push_record(nodes.take_live(entry.payload)),
                EntryKind::HiddenGroup
                | EntryKind::HiddenValue
                | EntryKind::HiddenNode
                | EntryKind::Unused => entry.payload,
            };
        }

        self.groups = rebuilt_groups;
        self.values = rebuilt_values;
        self.nodes = rebuilt_nodes;
    }

    fn should_compact(
        old_len: usize,
        hidden_count: usize,
        new_len: usize,
        eager_compact_slot_len: usize,
        fractional_compact_gap_threshold: usize,
        fractional_compact_ratio_divisor: usize,
    ) -> bool {
        if old_len <= eager_compact_slot_len {
            return true;
        }

        hidden_count >= new_len
            || (hidden_count >= fractional_compact_gap_threshold
                && hidden_count.saturating_mul(fractional_compact_ratio_divisor) >= old_len)
    }
}

struct CompactionFrame {
    group: u32,
    end: usize,
    kept_before: usize,
}

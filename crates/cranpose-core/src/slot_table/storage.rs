use super::{
    anchor_map::AnchorMap,
    lifecycle_queue::{DeferredDrop, OrphanedNode},
    GroupRetention, NodeSlotState, SlotTableDebugStats, SlotValueTypeDebugStat,
};
use crate::{AnchorId, Key, NodeId, RecomposeScope};
use std::any::Any;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryClass {
    Group,
    Value,
    Node,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryVisibility {
    Live,
    Hidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryKind {
    Unused,
    Occupied {
        class: EntryClass,
        visibility: EntryVisibility,
    },
}

impl EntryKind {
    pub(crate) fn live(class: EntryClass) -> Self {
        Self::Occupied {
            class,
            visibility: EntryVisibility::Live,
        }
    }

    pub(crate) fn hidden(class: EntryClass) -> Self {
        Self::Occupied {
            class,
            visibility: EntryVisibility::Hidden,
        }
    }

    pub(crate) fn class(self) -> Option<EntryClass> {
        match self {
            Self::Unused => None,
            Self::Occupied { class, .. } => Some(class),
        }
    }

    pub(crate) fn visibility(self) -> Option<EntryVisibility> {
        match self {
            Self::Unused => None,
            Self::Occupied { visibility, .. } => Some(visibility),
        }
    }

    pub(crate) fn is_group(self) -> bool {
        self.class() == Some(EntryClass::Group)
    }

    pub(crate) fn is_hidden(self) -> bool {
        self.visibility() == Some(EntryVisibility::Hidden)
    }

    pub(crate) fn restore(self) -> Self {
        match self {
            Self::Occupied { class, .. } => Self::live(class),
            other => other,
        }
    }

    pub(crate) fn hide(self) -> Self {
        match self {
            Self::Occupied { class, .. } => Self::hidden(class),
            other => other,
        }
    }

    pub(crate) fn matches(self, class: EntryClass, visibility: EntryVisibility) -> bool {
        matches!(
            self,
            Self::Occupied {
                class: entry_class,
                visibility: entry_visibility,
            } if entry_class == class && entry_visibility == visibility
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EntryDescriptor {
    pub(crate) kind: EntryKind,
    pub(crate) anchor: AnchorId,
    pub(crate) payload: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GroupSpans {
    pub(crate) live_extent: u32,
    pub(crate) scan_extent: u32,
}

impl GroupSpans {
    pub(crate) fn new(live_extent: usize, scan_extent: usize) -> Self {
        Self {
            live_extent: live_extent as u32,
            scan_extent: scan_extent as u32,
        }
    }

    pub(crate) fn live_extent(self) -> usize {
        self.live_extent as usize
    }

    pub(crate) fn scan_extent(self) -> usize {
        self.scan_extent as usize
    }
}

#[derive(Clone)]
pub(crate) struct GroupSnapshot {
    pub(crate) key: Key,
    pub(crate) spans: GroupSpans,
    pub(crate) scope: Option<RecomposeScope>,
}

#[derive(Default)]
pub(crate) struct StorageEffects {
    deactivated_scopes: Vec<RecomposeScope>,
    orphaned_nodes: Vec<OrphanedNode>,
}

impl StorageEffects {
    pub(crate) fn deactivate_scope(&mut self, scope: RecomposeScope) {
        self.deactivated_scopes.push(scope);
    }

    pub(crate) fn orphan_node(&mut self, orphaned: OrphanedNode) {
        self.orphaned_nodes.push(orphaned);
    }

    pub(crate) fn take_deactivated_scopes(&mut self) -> Vec<RecomposeScope> {
        std::mem::take(&mut self.deactivated_scopes)
    }

    pub(crate) fn take_orphaned_nodes(&mut self) -> Vec<OrphanedNode> {
        std::mem::take(&mut self.orphaned_nodes)
    }
}

pub(crate) struct HideRangeResult {
    pub(crate) effects: StorageEffects,
    pub(crate) hidden_group_roots: Vec<(usize, usize)>,
    pub(crate) top_level_hidden_group_roots: Vec<usize>,
    pub(crate) marked_any: bool,
    pub(crate) newly_hidden_entries: usize,
}

struct GroupRecord {
    key: Key,
    spans: GroupSpans,
    retention: GroupRetention,
    parent_anchor: AnchorId,
    hidden_descendants: u32,
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
    live_extents: Vec<u32>,
    scan_extents: Vec<u32>,
    retentions: Vec<GroupRetention>,
    parent_anchors: Vec<AnchorId>,
    hidden_descendants: Vec<u32>,
    scopes: Vec<Option<RecomposeScope>>,
    free: Vec<u32>,
}

impl GroupArena {
    fn push_record(&mut self, record: GroupRecord) -> u32 {
        let id = self.keys.len() as u32;
        self.keys.push(record.key);
        self.live_extents.push(record.spans.live_extent);
        self.scan_extents.push(record.spans.scan_extent);
        self.retentions.push(record.retention);
        self.parent_anchors.push(record.parent_anchor);
        self.hidden_descendants.push(record.hidden_descendants);
        self.scopes.push(record.scope);
        id
    }

    fn alloc(
        &mut self,
        key: Key,
        spans: GroupSpans,
        retention: GroupRetention,
        parent_anchor: AnchorId,
        scope: Option<RecomposeScope>,
    ) -> u32 {
        if let Some(id) = self.free.pop() {
            let idx = id as usize;
            self.keys[idx] = key;
            self.live_extents[idx] = spans.live_extent;
            self.scan_extents[idx] = spans.scan_extent;
            self.retentions[idx] = retention;
            self.parent_anchors[idx] = parent_anchor;
            self.hidden_descendants[idx] = 0;
            self.scopes[idx] = scope;
            id
        } else {
            let id = self.keys.len() as u32;
            self.keys.push(key);
            self.live_extents.push(spans.live_extent);
            self.scan_extents.push(spans.scan_extent);
            self.retentions.push(retention);
            self.parent_anchors.push(parent_anchor);
            self.hidden_descendants.push(0);
            self.scopes.push(scope);
            id
        }
    }

    fn snapshot(&self, id: u32) -> GroupSnapshot {
        let idx = id as usize;
        GroupSnapshot {
            key: self.keys[idx],
            spans: GroupSpans {
                live_extent: self.live_extents[idx],
                scan_extent: self.scan_extents[idx],
            },
            scope: self.scopes[idx].clone(),
        }
    }

    fn key(&self, id: u32) -> Key {
        self.keys[id as usize]
    }

    fn live_extent(&self, id: u32) -> usize {
        self.live_extents[id as usize] as usize
    }

    fn scan_extent(&self, id: u32) -> usize {
        self.scan_extents[id as usize] as usize
    }

    fn set_spans(&mut self, id: u32, spans: GroupSpans) {
        self.live_extents[id as usize] = spans.live_extent;
        self.scan_extents[id as usize] = spans.scan_extent;
    }

    fn retention(&self, id: u32) -> GroupRetention {
        self.retentions[id as usize]
    }

    fn set_retention(&mut self, id: u32, retention: GroupRetention) {
        self.retentions[id as usize] = retention;
    }

    fn parent_anchor(&self, id: u32) -> AnchorId {
        self.parent_anchors[id as usize]
    }

    fn set_parent_anchor(&mut self, id: u32, parent_anchor: AnchorId) {
        self.parent_anchors[id as usize] = parent_anchor;
    }

    fn hidden_descendants(&self, id: u32) -> usize {
        self.hidden_descendants[id as usize] as usize
    }

    fn set_hidden_descendants(&mut self, id: u32, hidden_descendants: usize) {
        self.hidden_descendants[id as usize] = hidden_descendants as u32;
    }

    fn adjust_hidden_descendants(&mut self, id: u32, delta: isize) {
        let idx = id as usize;
        let next = self.hidden_descendants[idx] as i64 + delta as i64;
        assert!(
            next >= 0,
            "group hidden descendant count underflow: id={id} delta={delta}"
        );
        self.hidden_descendants[idx] = next as u32;
    }

    fn scope(&self, id: u32) -> Option<&RecomposeScope> {
        self.scopes[id as usize].as_ref()
    }

    fn set_scope(&mut self, id: u32, scope: Option<RecomposeScope>) {
        self.scopes[id as usize] = scope;
    }

    #[cfg(test)]
    fn clear_scope(&mut self, id: u32) {
        let idx = id as usize;
        let _ = self.scopes[idx].take();
    }

    fn take_drop(&mut self, id: u32) -> DeferredDrop {
        let idx = id as usize;
        let scope = self.scopes[idx].take();
        self.keys[idx] = 0;
        self.live_extents[idx] = 0;
        self.scan_extents[idx] = 0;
        self.retentions[idx] = GroupRetention::new(0);
        self.parent_anchors[idx] = AnchorId::INVALID;
        self.hidden_descendants[idx] = 0;
        self.free.push(id);
        DeferredDrop::Group(scope)
    }

    fn take_live(&mut self, id: u32) -> GroupRecord {
        let idx = id as usize;
        GroupRecord {
            key: std::mem::take(&mut self.keys[idx]),
            spans: GroupSpans {
                live_extent: std::mem::take(&mut self.live_extents[idx]),
                scan_extent: std::mem::take(&mut self.scan_extents[idx]),
            },
            retention: std::mem::replace(&mut self.retentions[idx], GroupRetention::new(0)),
            parent_anchor: std::mem::replace(&mut self.parent_anchors[idx], AnchorId::INVALID),
            hidden_descendants: std::mem::take(&mut self.hidden_descendants[idx]),
            scope: self.scopes[idx].take(),
        }
    }

    fn heap_bytes(&self) -> usize {
        self.keys.capacity() * std::mem::size_of::<Key>()
            + self.live_extents.capacity() * std::mem::size_of::<u32>()
            + self.scan_extents.capacity() * std::mem::size_of::<u32>()
            + self.retentions.capacity() * std::mem::size_of::<GroupRetention>()
            + self.parent_anchors.capacity() * std::mem::size_of::<AnchorId>()
            + self.hidden_descendants.capacity() * std::mem::size_of::<u32>()
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
                if physical >= self.anchors.len() {
                    break;
                }
                let anchor = self.anchors[physical];
                anchor_map.update_for_gap_move(anchor, logical, index, self.len);
            }
            let end = (index + self.gap_len).min(self.kinds.len());
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
    anchor_map: AnchorMap,
    hidden_count: usize,
    pub(crate) needs_compact: bool,
}

impl SlotStorage {
    pub(crate) fn new() -> Self {
        Self {
            buffer: EntryBuffer::new(),
            groups: GroupArena::default(),
            values: ValueArena::default(),
            nodes: NodeArena::default(),
            anchor_map: AnchorMap::default(),
            hidden_count: 0,
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
            + self.anchor_map.debug_heap_bytes()
    }

    pub(crate) fn debug_stats(&self) -> SlotTableDebugStats {
        let mut stats = SlotTableDebugStats {
            slots_len: self.buffer.len(),
            slots_cap: self.buffer.capacity(),
            pending_slot_drops_len: 0,
            pending_slot_drops_cap: 0,
            anchors_len: 0,
            anchors_cap: 0,
            hidden_entries_len: self.hidden_count,
            preserved_groups_len: self.preserved_group_count(),
            free_anchor_ids_len: 0,
            free_anchor_ids_cap: 0,
            group_stack_len: 0,
            group_stack_cap: 0,
            orphaned_node_ids_len: 0,
            orphaned_node_ids_cap: 0,
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

    pub(crate) fn entry_scan_extent(&self, index: usize) -> usize {
        match self.entry(index).map(|entry| entry.kind) {
            Some(kind) if kind.is_group() => self.group_extent_at(index),
            Some(EntryKind::Occupied { .. }) => 1,
            Some(EntryKind::Unused) | None => 0,
        }
    }

    pub(crate) fn group_id_at(&self, index: usize) -> Option<u32> {
        let entry = self.entry(index)?;
        entry.kind.is_group().then_some(entry.payload)
    }

    pub(crate) fn live_group_id_at(&self, index: usize) -> Option<u32> {
        let entry = self.entry(index)?;
        entry
            .kind
            .matches(EntryClass::Group, EntryVisibility::Live)
            .then_some(entry.payload)
    }

    pub(crate) fn group_snapshot_at(&self, index: usize) -> Option<GroupSnapshot> {
        let group = self.group_id_at(index)?;
        Some(self.groups.snapshot(group))
    }

    pub(crate) fn group_extent_at(&self, index: usize) -> usize {
        self.group_scan_extent_at(index)
    }

    pub(crate) fn group_live_extent_at(&self, index: usize) -> usize {
        self.group_id_at(index)
            .map(|group| self.groups.live_extent(group))
            .unwrap_or(0)
    }

    pub(crate) fn group_scan_extent_at(&self, index: usize) -> usize {
        self.group_id_at(index)
            .map(|group| self.groups.scan_extent(group))
            .unwrap_or(0)
    }

    pub(crate) fn group_key_at(&self, index: usize) -> Option<Key> {
        self.group_id_at(index).map(|group| self.groups.key(group))
    }

    pub(crate) fn group_retention_at(&self, index: usize) -> Option<GroupRetention> {
        self.group_id_at(index)
            .map(|group| self.groups.retention(group))
    }

    pub(crate) fn group_hidden_descendants_at(&self, index: usize) -> usize {
        self.group_id_at(index)
            .map(|group| self.groups.hidden_descendants(group))
            .unwrap_or(0)
    }

    pub(crate) fn live_group_scope(&self, index: usize) -> Option<&RecomposeScope> {
        let group = self.live_group_id_at(index)?;
        self.groups.scope(group)
    }

    pub(crate) fn live_group_has_scope(&self, index: usize) -> bool {
        self.live_group_scope(index).is_some()
    }

    #[cfg(test)]
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

    pub(crate) fn set_group_spans(&mut self, index: usize, spans: GroupSpans) {
        if let Some(group) = self.group_id_at(index) {
            self.groups.set_spans(group, spans);
        }
    }

    pub(crate) fn set_group_retention(&mut self, index: usize, retention: GroupRetention) {
        if let Some(group) = self.group_id_at(index) {
            self.groups.set_retention(group, retention);
        }
    }

    pub(crate) fn set_group_hidden_descendants(&mut self, index: usize, hidden_descendants: usize) {
        if let Some(group) = self.group_id_at(index) {
            self.groups
                .set_hidden_descendants(group, hidden_descendants);
        }
    }

    pub(crate) fn adjust_group_hidden_descendants(&mut self, index: usize, delta: isize) {
        if let Some(group) = self.group_id_at(index) {
            self.groups.adjust_hidden_descendants(group, delta);
        }
    }

    pub(crate) fn group_parent_anchor_at(&self, index: usize) -> Option<AnchorId> {
        self.group_id_at(index)
            .map(|group| self.groups.parent_anchor(group))
    }

    pub(crate) fn set_group_parent_anchor(&mut self, index: usize, parent_anchor: AnchorId) {
        if let Some(group) = self.group_id_at(index) {
            self.groups.set_parent_anchor(group, parent_anchor);
        }
    }

    pub(crate) fn alloc_group(
        &mut self,
        key: Key,
        retention: GroupRetention,
        parent_anchor: AnchorId,
        scope: Option<RecomposeScope>,
    ) -> u32 {
        self.groups
            .alloc(key, GroupSpans::new(0, 0), retention, parent_anchor, scope)
    }

    pub(crate) fn insert_group(
        &mut self,
        index: usize,
        anchor: AnchorId,
        group: u32,
        hidden: bool,
    ) {
        let kind = if hidden {
            EntryKind::hidden(EntryClass::Group)
        } else {
            EntryKind::live(EntryClass::Group)
        };
        self.buffer.insert_entry(
            index,
            EntryDescriptor {
                kind,
                anchor,
                payload: group,
            },
            &mut self.anchor_map,
        );
        self.adjust_hidden_count_for_insert(kind);
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
        let kind = if hidden {
            EntryKind::hidden(EntryClass::Value)
        } else {
            EntryKind::live(EntryClass::Value)
        };
        self.buffer.insert_entry(
            index,
            EntryDescriptor {
                kind,
                anchor,
                payload: value,
            },
            &mut self.anchor_map,
        );
        self.adjust_hidden_count_for_insert(kind);
    }

    pub(crate) fn overwrite_value(
        &mut self,
        index: usize,
        anchor: AnchorId,
        value: u32,
        hidden: bool,
    ) -> Option<DeferredDrop> {
        let dropped = self.take_entry_payload_for_overwrite(index);
        let kind = if hidden {
            EntryKind::hidden(EntryClass::Value)
        } else {
            EntryKind::live(EntryClass::Value)
        };
        self.buffer.set_entry(
            index,
            EntryDescriptor {
                kind,
                anchor,
                payload: value,
            },
        );
        self.anchor_map
            .register_anchor(anchor, index, self.buffer.gap_start, self.buffer.len());
        self.adjust_hidden_count_for_insert(kind);
        dropped
    }

    pub(crate) fn value_matches_type<T: 'static>(&self, index: usize) -> bool {
        let Some(entry) = self.entry(index) else {
            return false;
        };
        matches!(entry.kind.class(), Some(EntryClass::Value))
            && self.values.is_type::<T>(entry.payload)
    }

    pub(crate) fn read_value<T: 'static>(&self, index: usize) -> &T {
        let entry = self.entry(index).expect("slot index out of bounds");
        debug_assert!(
            matches!(entry.kind.class(), Some(EntryClass::Value)),
            "slot is not a value"
        );
        self.values.read(entry.payload)
    }

    pub(crate) fn read_value_mut<T: 'static>(&mut self, index: usize) -> &mut T {
        let entry = self.entry(index).expect("slot index out of bounds");
        debug_assert!(
            matches!(entry.kind.class(), Some(EntryClass::Value)),
            "slot is not a value"
        );
        self.values.read_mut(entry.payload)
    }

    pub(crate) fn write_value<T: 'static>(&mut self, index: usize, value: T) {
        let entry = self.entry(index).expect("slot index out of bounds");
        debug_assert!(
            matches!(entry.kind.class(), Some(EntryClass::Value)),
            "slot is not a value"
        );
        self.values.write(entry.payload, value);
    }

    pub(crate) fn alloc_node(&mut self, id: NodeId, generation: u32) -> u32 {
        self.nodes.alloc(id, generation)
    }

    pub(crate) fn insert_node(&mut self, index: usize, anchor: AnchorId, node: u32, hidden: bool) {
        let kind = if hidden {
            EntryKind::hidden(EntryClass::Node)
        } else {
            EntryKind::live(EntryClass::Node)
        };
        self.buffer.insert_entry(
            index,
            EntryDescriptor {
                kind,
                anchor,
                payload: node,
            },
            &mut self.anchor_map,
        );
        self.adjust_hidden_count_for_insert(kind);
    }

    pub(crate) fn overwrite_node(
        &mut self,
        index: usize,
        anchor: AnchorId,
        node: u32,
        hidden: bool,
    ) -> Option<DeferredDrop> {
        let dropped = self.take_entry_payload_for_overwrite(index);
        let kind = if hidden {
            EntryKind::hidden(EntryClass::Node)
        } else {
            EntryKind::live(EntryClass::Node)
        };
        self.buffer.set_entry(
            index,
            EntryDescriptor {
                kind,
                anchor,
                payload: node,
            },
        );
        self.anchor_map
            .register_anchor(anchor, index, self.buffer.gap_start, self.buffer.len());
        self.adjust_hidden_count_for_insert(kind);
        dropped
    }

    pub(crate) fn node_at(&self, index: usize) -> Option<(EntryKind, NodeId, u32)> {
        let entry = self.entry(index)?;
        matches!(entry.kind.class(), Some(EntryClass::Node)).then(|| {
            let (id, generation) = self.nodes.get(entry.payload);
            (entry.kind, id, generation)
        })
    }

    fn hide_entry_into(&mut self, index: usize, effects: &mut StorageEffects) -> bool {
        let Some(entry) = self.entry(index) else {
            return false;
        };
        let Some(class) = entry.kind.class() else {
            return false;
        };
        if entry.kind.is_hidden() {
            return false;
        }

        self.buffer.set_kind(index, entry.kind.hide());
        self.hidden_count += 1;
        match class {
            EntryClass::Group => {
                if let Some(scope) = self.groups.scope(entry.payload).cloned() {
                    effects.deactivate_scope(scope);
                }
            }
            EntryClass::Value => {}
            EntryClass::Node => {
                let (id, generation) = self.nodes.get(entry.payload);
                effects.orphan_node(OrphanedNode::new(id, generation, entry.anchor));
            }
        }
        true
    }

    pub(crate) fn hide_range(&mut self, start: usize, end: usize) -> HideRangeResult {
        let end = end.min(self.len());
        let mut effects = StorageEffects::default();
        let mut hidden_group_roots = Vec::new();
        let mut top_level_hidden_group_roots = Vec::new();
        let mut marked_any = false;
        let mut newly_hidden_entries = 0usize;
        let mut index = start;

        while index < end {
            let Some(kind) = self.entry_kind(index) else {
                break;
            };
            let extent = self.entry_scan_extent(index).max(1);
            match kind {
                EntryKind::Occupied {
                    class: EntryClass::Group,
                    visibility: EntryVisibility::Live,
                } => {
                    let subtree_end = (index + extent).min(end);
                    top_level_hidden_group_roots.push(index);
                    for child in index..subtree_end {
                        if self.entry_kind(child) == Some(EntryKind::live(EntryClass::Group)) {
                            hidden_group_roots.push((child, self.entry_scan_extent(child).max(1)));
                        }
                        if self.hide_entry_into(child, &mut effects) {
                            marked_any = true;
                            newly_hidden_entries += 1;
                        }
                    }
                    index = subtree_end;
                }
                EntryKind::Occupied {
                    class: EntryClass::Value | EntryClass::Node,
                    visibility: EntryVisibility::Live,
                } => {
                    if self.hide_entry_into(index, &mut effects) {
                        marked_any = true;
                        newly_hidden_entries += 1;
                    }
                    index += 1;
                }
                EntryKind::Occupied {
                    visibility: EntryVisibility::Hidden,
                    ..
                } => {
                    index += extent;
                }
                EntryKind::Unused => {
                    index += 1;
                }
            }
        }

        if marked_any {
            self.needs_compact = true;
        }

        HideRangeResult {
            effects,
            hidden_group_roots,
            top_level_hidden_group_roots,
            marked_any,
            newly_hidden_entries,
        }
    }

    pub(crate) fn restore_hidden_entry(&mut self, index: usize) {
        if let Some(entry) = self.entry(index) {
            if entry.kind.is_hidden() {
                self.decrement_hidden_count();
            }
            self.buffer.set_kind(index, entry.kind.restore());
        }
    }

    fn take_entry_payload_for_overwrite(&mut self, index: usize) -> Option<DeferredDrop> {
        let entry = self.entry(index)?;
        if entry.kind.is_hidden() {
            self.decrement_hidden_count();
        }

        match entry.kind.class() {
            Some(EntryClass::Value) => Some(self.values.take_drop(entry.payload)),
            Some(EntryClass::Node) => {
                self.nodes.free(entry.payload);
                None
            }
            Some(EntryClass::Group) | None => {
                panic!("overwrite path encountered non-value/node entry at slot {index}")
            }
        }
    }

    pub(crate) fn orphaned_node_state(&self, orphaned: OrphanedNode) -> NodeSlotState {
        let Some(index) = self.resolve_anchor(orphaned.anchor) else {
            return NodeSlotState::Missing;
        };

        match self.node_at(index) {
            Some((
                EntryKind::Occupied {
                    class: EntryClass::Node,
                    visibility: EntryVisibility::Live,
                },
                id,
                generation,
            )) if id == orphaned.id && generation == orphaned.generation => NodeSlotState::Active,
            Some((
                EntryKind::Occupied {
                    class: EntryClass::Node,
                    visibility: EntryVisibility::Hidden,
                },
                id,
                generation,
            )) if id == orphaned.id && generation == orphaned.generation => {
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
    ) -> Vec<DeferredDrop> {
        if !self.needs_compact {
            return Vec::new();
        }

        let old_len = self.buffer.len();
        if old_len == 0 {
            self.needs_compact = false;
            return Vec::new();
        }

        let hidden_count = self.hidden_count;
        if hidden_count == 0 {
            self.needs_compact = false;
            return Vec::new();
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
            return Vec::new();
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
                let live_extent = kept.saturating_sub(frame.kept_before);
                self.groups
                    .set_spans(frame.group, GroupSpans::new(live_extent, live_extent));
                self.groups.set_hidden_descendants(frame.group, 0);
            }

            let entry = self.entry(index).expect("entry during compaction");
            if entry.kind.is_hidden() {
                if entry
                    .kind
                    .matches(EntryClass::Group, EntryVisibility::Hidden)
                {
                    let hidden_len = self.group_scan_extent_at(index);
                    for hidden_index in index..index + hidden_len {
                        let hidden_entry = self.entry(hidden_index).expect("hidden entry");
                        if let Some(deferred_drop) = self.collect_removed_entry(hidden_entry) {
                            removed.push(deferred_drop);
                        }
                    }
                    index += hidden_len;
                } else {
                    if let Some(deferred_drop) = self.collect_removed_entry(entry) {
                        removed.push(deferred_drop);
                    }
                    index += 1;
                }
                continue;
            }

            if entry.kind.matches(EntryClass::Group, EntryVisibility::Live) {
                stack.push(CompactionFrame {
                    group: entry.payload,
                    end: index + self.group_scan_extent_at(index),
                    kept_before: kept,
                });
            }

            survivors.push(entry);
            kept += 1;
            index += 1;
        }

        while let Some(frame) = stack.pop() {
            let live_extent = kept.saturating_sub(frame.kept_before);
            self.groups
                .set_spans(frame.group, GroupSpans::new(live_extent, live_extent));
            self.groups.set_hidden_descendants(frame.group, 0);
        }

        self.rebuild_live_arenas(&mut survivors);
        self.buffer.reset_from_live_entries(&survivors);
        self.hidden_count = 0;
        self.rebuild_anchor_positions();
        removed
    }

    pub(crate) fn debug_value_type_counts(&self, limit: usize) -> Vec<SlotValueTypeDebugStat> {
        let mut counts =
            crate::collections::map::HashMap::<&'static str, (usize, usize)>::default();
        for index in 0..self.buffer.len() {
            let Some(entry) = self.entry(index) else {
                continue;
            };
            match entry.kind.class() {
                Some(EntryClass::Group) => {
                    if self.groups.scope(entry.payload).is_some() {
                        let name = std::any::type_name::<RecomposeScope>();
                        let bytes = std::mem::size_of::<RecomposeScope>();
                        let count = counts.entry(name).or_insert((0, 0));
                        count.0 += 1;
                        count.1 += bytes;
                    }
                }
                Some(EntryClass::Value) => {
                    let stat = self.values.type_debug_stat(entry.payload);
                    let count = counts.entry(stat.type_name).or_insert((0, 0));
                    count.0 += 1;
                    count.1 += stat.inline_payload_bytes;
                }
                Some(EntryClass::Node) | None => {}
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

    pub(crate) fn drop_all_reverse(&mut self) -> Vec<DeferredDrop> {
        let mut drops = Vec::with_capacity(self.buffer.len());
        for index in 0..self.buffer.len() {
            let entry = self.entry(index).expect("entry during drop");
            if let Some(deferred_drop) = self.collect_removed_entry(entry) {
                drops.push(deferred_drop);
            }
        }
        self.buffer.reset_from_live_entries(&[]);
        self.groups = GroupArena::default();
        self.values = ValueArena::default();
        self.nodes = NodeArena::default();
        self.hidden_count = 0;
        self.anchor_map
            .rebuild_positions(std::iter::empty::<(AnchorId, usize)>(), 0, 0);
        drops
    }

    fn collect_removed_entry(&mut self, entry: EntryDescriptor) -> Option<DeferredDrop> {
        let deferred_drop = match entry.kind.class() {
            Some(EntryClass::Group) => Some(self.groups.take_drop(entry.payload)),
            Some(EntryClass::Value) => Some(self.values.take_drop(entry.payload)),
            Some(EntryClass::Node) => {
                self.nodes.free(entry.payload);
                None
            }
            None => None,
        };
        if entry.anchor.is_valid() {
            self.anchor_map.free_anchor(entry.anchor);
        }
        deferred_drop
    }

    fn rebuild_live_arenas(&mut self, survivors: &mut [EntryDescriptor]) {
        let mut groups = std::mem::take(&mut self.groups);
        let mut values = std::mem::take(&mut self.values);
        let mut nodes = std::mem::take(&mut self.nodes);

        let mut rebuilt_groups = GroupArena::default();
        let mut rebuilt_values = ValueArena::default();
        let mut rebuilt_nodes = NodeArena::default();

        for entry in survivors {
            entry.payload = match entry.kind.class() {
                Some(EntryClass::Group) => {
                    rebuilt_groups.push_record(groups.take_live(entry.payload))
                }
                Some(EntryClass::Value) => {
                    rebuilt_values.push_record(values.take_live(entry.payload))
                }
                Some(EntryClass::Node) => rebuilt_nodes.push_record(nodes.take_live(entry.payload)),
                None => entry.payload,
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

    fn adjust_hidden_count_for_insert(&mut self, kind: EntryKind) {
        if kind.is_hidden() {
            self.hidden_count += 1;
        }
    }

    fn preserved_group_count(&self) -> usize {
        let mut preserved = 0usize;
        for index in 0..self.buffer.len() {
            if self.entry_kind(index) == Some(EntryKind::hidden(EntryClass::Group)) {
                preserved += 1;
            }
        }
        preserved
    }

    fn decrement_hidden_count(&mut self) {
        self.hidden_count = self
            .hidden_count
            .checked_sub(1)
            .expect("hidden entry count underflow");
    }
}

struct CompactionFrame {
    group: u32,
    end: usize,
    kept_before: usize,
}

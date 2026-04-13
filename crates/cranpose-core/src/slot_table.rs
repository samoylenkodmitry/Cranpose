//! Slot table implementation using a gap-buffer strategy.
//!
//! This is the baseline/reference slot storage implementation that provides:
//! - Gap-based slot reuse during conditional rendering
//! - Anchor-based positional stability during reorganization
//! - Efficient group skipping and scope-based recomposition
//! - Batch anchor rebuilding for large structural changes

// Complex slot state machine logic benefits from explicit nested pattern matching for clarity
#![allow(clippy::collapsible_match)]

use crate::{
    collections::map::HashMap,
    slot_storage::{GroupId, SlotStorage, StartGroup, ValueSlotId},
    AnchorId, Key, NodeId, Owned, RecomposeScope, ScopeId,
};
use std::any::{Any, TypeId};
use std::cell::Cell;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
struct PackedScopeId(u64);

impl PackedScopeId {
    fn new(scope: Option<ScopeId>) -> Self {
        let packed = scope.map(|scope| scope as u64).unwrap_or(0);
        Self(packed)
    }

    fn to_option(self) -> Option<ScopeId> {
        match self.0 {
            0 => None,
            scope => Some(scope as ScopeId),
        }
    }
}

fn pack_slot_len(len: usize) -> u32 {
    u32::try_from(len).expect("slot length overflow")
}

fn unpack_slot_len(len: u32) -> usize {
    len as usize
}

pub struct SlotTable {
    slots: Vec<Slot>, // FUTURE(no_std): replace Vec with arena-backed slot storage.
    pending_slot_drops: PendingSlotDrops,
    cursor: usize,
    group_stack: Vec<GroupFrame>, // FUTURE(no_std): switch to small stack buffer.
    /// Maps anchor IDs to their current physical positions in the slots array.
    /// This indirection layer provides positional stability during slot reorganization.
    anchors: HashMap<usize, usize>, // key = anchor_id.0, value = physical slot position
    gap_metadata: HashMap<usize, GapMetadata>,
    anchors_dirty: bool,
    /// Counter for allocating unique anchor IDs.
    next_anchor_id: Cell<usize>,
    /// Freed anchor IDs available for reuse, preventing unbounded anchors Vec growth.
    free_anchor_ids: Vec<usize>,
    /// Tracks whether the most recent start() reused a gap slot.
    last_start_was_gap: bool,
    /// Node IDs orphaned when their slots were converted to gaps.
    /// The composer drains these to issue RemoveNode commands and free nodes.
    orphaned_node_ids: OrphanedNodeIds,
    /// Set when structural changes require compaction (gap marking, etc.).
    needs_compact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableDebugStats {
    pub slots_len: usize,
    pub slots_cap: usize,
    pub pending_slot_drops_len: usize,
    pub pending_slot_drops_cap: usize,
    pub anchors_len: usize,
    pub anchors_cap: usize,
    pub free_anchor_ids_len: usize,
    pub free_anchor_ids_cap: usize,
    pub group_stack_len: usize,
    pub group_stack_cap: usize,
    pub orphaned_node_ids_len: usize,
    pub orphaned_node_ids_cap: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotValueTypeDebugStat {
    pub type_name: &'static str,
    pub count: usize,
    pub inline_payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GapMetadata {
    group_key: Option<Key>,
    boundary_key: Option<Key>,
    group_scope: PackedScopeId,
    group_len: u32,
    preserved_node: Option<(NodeId, u32)>,
}

enum Slot {
    Group {
        key: Key,
        anchor: AnchorId,
        len: u32,
        scope: PackedScopeId,
        boundary_key: Key,
        has_gap_children: bool,
    },
    Value {
        anchor: AnchorId,
        data: Box<dyn SlotValue>,
    },
    ScopeValue {
        anchor: AnchorId,
        scope: RecomposeScope,
    },
    Node {
        anchor: AnchorId,
        id: NodeId,
        gen: u32,
    },
    /// Gap: Marks an unused slot that can be reused or compacted.
    /// This prevents destructive truncation that would destroy sibling components.
    /// For Groups marked as gaps (e.g., during tab switching), we preserve their
    /// key, scope, and length so they can be properly matched and reused when reactivated.
    Gap { anchor: AnchorId },
}

struct GroupFrame {
    key: Key,
    start: usize, // Physical position (will be phased out)
    end: usize,   // Physical position (will be phased out)
    child_reuse: ChildReusePolicy,
    fresh_body: bool,
    gap_boundary_key: Key,
}

struct GroupCompactionFrame {
    index: usize,
    end: usize,
    kept_before: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ChildReusePolicy {
    Normal,
    ParentRestoredFromGap,
    FreshInsert,
}

impl ChildReusePolicy {
    fn requires_restricted_reuse(self) -> bool {
        !matches!(self, Self::Normal)
    }

    fn allows_exact_live_reuse(self) -> bool {
        !matches!(self, Self::FreshInsert)
    }
}

fn restored_child_reuse(parent_reuse: ChildReusePolicy) -> ChildReusePolicy {
    if matches!(parent_reuse, ChildReusePolicy::FreshInsert) {
        ChildReusePolicy::FreshInsert
    } else {
        ChildReusePolicy::ParentRestoredFromGap
    }
}

trait SlotValue: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn rebox(self: Box<Self>) -> Box<dyn SlotValue>;
    fn debug_type_name(&self) -> &'static str;
    fn debug_inline_payload_bytes(&self) -> usize;
}

struct TypedSlotValue<T: Any>(T);

impl<T: Any> SlotValue for TypedSlotValue<T> {
    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.0
    }

    fn rebox(self: Box<Self>) -> Box<dyn SlotValue> {
        Box::new(TypedSlotValue(self.0))
    }

    fn debug_type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }

    fn debug_inline_payload_bytes(&self) -> usize {
        std::mem::size_of::<T>()
    }
}

const INVALID_ANCHOR_POS: usize = usize::MAX;

#[derive(Default)]
struct ChunkedPending<T> {
    sealed: Vec<Vec<T>>,
    active: Vec<T>,
    len: usize,
}

impl<T> ChunkedPending<T> {
    fn len(&self) -> usize {
        self.len
    }

    fn capacity(&self) -> usize {
        self.active.capacity() + self.sealed.iter().map(Vec::capacity).sum::<usize>()
    }

    fn push(&mut self, value: T, initial_capacity: usize, chunk_capacity: usize) {
        if self.active.capacity() == 0 {
            self.active = Vec::with_capacity(initial_capacity);
        } else if self.active.len() == self.active.capacity() {
            let next_capacity = chunk_capacity.max(initial_capacity);
            let full_chunk = std::mem::replace(&mut self.active, Vec::with_capacity(next_capacity));
            self.sealed.push(full_chunk);
        }

        self.active.push(value);
        self.len += 1;
    }

    fn clear_and_drop_reverse(&mut self) {
        while let Some(value) = self.active.pop() {
            drop(value);
        }

        while let Some(mut chunk) = self.sealed.pop() {
            while let Some(value) = chunk.pop() {
                drop(value);
            }
        }

        self.len = 0;
    }

    fn drain_forward(&mut self, mut visitor: impl FnMut(T)) {
        let sealed = std::mem::take(&mut self.sealed);
        for chunk in sealed {
            for value in chunk {
                visitor(value);
            }
        }
        let active = std::mem::take(&mut self.active);
        for value in active {
            visitor(value);
        }
        self.len = 0;
    }

    fn trim_retained_capacity(&mut self, retained: usize, initial_capacity: usize) {
        self.sealed = Vec::new();
        if self.active.capacity() > retained.saturating_mul(4) {
            self.active = Vec::with_capacity(retained.max(initial_capacity));
        }
    }
}

#[derive(Default)]
struct PendingSlotDrops {
    inner: ChunkedPending<Slot>,
}

impl PendingSlotDrops {
    const CHUNK_CAPACITY: usize = 1024;
    const INITIAL_CAPACITY: usize = 4;

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    fn push(&mut self, slot: Slot) {
        self.inner
            .push(slot, Self::INITIAL_CAPACITY, Self::CHUNK_CAPACITY);
    }

    fn clear_and_drop_reverse(&mut self) {
        let _teardown = crate::runtime::enter_state_teardown_scope();
        self.inner.clear_and_drop_reverse();
    }

    fn trim_retained_capacity(&mut self, retained: usize) {
        self.inner
            .trim_retained_capacity(retained, Self::INITIAL_CAPACITY);
    }
}

#[derive(Default)]
struct OrphanedNodeIds {
    inner: ChunkedPending<OrphanedNode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrphanedNode {
    pub id: NodeId,
    pub generation: u32,
    anchor: AnchorId,
}

impl OrphanedNode {
    fn new(id: NodeId, generation: u32, anchor: AnchorId) -> Self {
        Self {
            id,
            generation,
            anchor,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeSlotState {
    Active,
    PreservedGap,
    Missing,
}

impl OrphanedNodeIds {
    const CHUNK_CAPACITY: usize = 1024;
    const INITIAL_CAPACITY: usize = 32;

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    fn push(&mut self, orphaned: OrphanedNode) {
        self.inner
            .push(orphaned, Self::INITIAL_CAPACITY, Self::CHUNK_CAPACITY);
    }

    fn drain_forward(&mut self, visitor: impl FnMut(OrphanedNode)) {
        self.inner.drain_forward(visitor);
        self.trim_retained_capacity(Self::INITIAL_CAPACITY);
    }

    fn trim_retained_capacity(&mut self, retained: usize) {
        self.inner
            .trim_retained_capacity(retained, Self::INITIAL_CAPACITY);
    }
}

#[derive(Debug, PartialEq)]
enum SlotKind {
    Group,
    Value,
    Node,
    Gap,
}

impl Slot {
    fn kind(&self) -> SlotKind {
        match self {
            Slot::Group { .. } => SlotKind::Group,
            Slot::Value { .. } => SlotKind::Value,
            Slot::ScopeValue { .. } => SlotKind::Value,
            Slot::Node { .. } => SlotKind::Node,
            Slot::Gap { .. } => SlotKind::Gap,
        }
    }

    /// Get the anchor ID for this slot.
    fn anchor_id(&self) -> AnchorId {
        match self {
            Slot::Group { anchor, .. } => *anchor,
            Slot::Value { anchor, .. } => *anchor,
            Slot::ScopeValue { anchor, .. } => *anchor,
            Slot::Node { anchor, .. } => *anchor,
            Slot::Gap { anchor, .. } => *anchor,
        }
    }

    fn as_value<T: 'static>(&self) -> &T {
        match self {
            Slot::Value { data, .. } => data
                .as_any()
                .downcast_ref::<T>()
                .expect("slot value type mismatch"),
            Slot::ScopeValue { scope, .. } => (scope as &dyn Any)
                .downcast_ref::<T>()
                .expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }

    fn as_value_mut<T: 'static>(&mut self) -> &mut T {
        match self {
            Slot::Value { data, .. } => data
                .as_any_mut()
                .downcast_mut::<T>()
                .expect("slot value type mismatch"),
            Slot::ScopeValue { scope, .. } => (scope as &mut dyn Any)
                .downcast_mut::<T>()
                .expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }

    fn deactivate_scope(&self) {
        if let Slot::ScopeValue { scope, .. } = self {
            scope.deactivate();
        }
    }
}

impl Default for Slot {
    fn default() -> Self {
        Slot::Group {
            key: 0,
            anchor: AnchorId::INVALID,
            len: 0,
            scope: PackedScopeId::default(),
            boundary_key: 0,
            has_gap_children: false,
        }
    }
}

fn drop_slots_in_reverse(slots: &mut Vec<Slot>) {
    let _teardown = crate::runtime::enter_state_teardown_scope();
    while let Some(slot) = slots.pop() {
        drop(slot);
    }
}

impl SlotTable {
    const INITIAL_CAP: usize = 32;
    const LOCAL_GAP_SCAN: usize = 256; // tune
    const EAGER_COMPACT_SLOT_LEN: usize = 1_024;
    const FRACTIONAL_COMPACT_GAP_THRESHOLD: usize = 256;
    const FRACTIONAL_COMPACT_RATIO_DIVISOR: usize = 4;
    const LARGE_GROWTH_THRESHOLD: usize = 32 * 1024;
    const LARGE_GROWTH_DIVISOR: usize = 4;
    const MIN_RETAINED_PENDING_SLOT_DROPS_CAPACITY: usize = 4;

    fn make_value_slot<T: 'static>(anchor: AnchorId, value: T) -> Slot {
        if TypeId::of::<T>() == TypeId::of::<RecomposeScope>() {
            let value: Box<dyn Any> = Box::new(value);
            let scope = *value
                .downcast::<RecomposeScope>()
                .expect("recompose scope slot type mismatch");
            Slot::ScopeValue { anchor, scope }
        } else {
            Slot::Value {
                anchor,
                data: Box::new(TypedSlotValue(value)),
            }
        }
    }

    fn rehouse_live_value_payloads(&mut self) {
        for slot in &mut self.slots {
            let moved = std::mem::take(slot);
            *slot = match moved {
                Slot::Value { anchor, data } => Slot::Value {
                    anchor,
                    data: data.rebox(),
                },
                other => other,
            };
        }
    }

    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            pending_slot_drops: PendingSlotDrops::default(),
            cursor: 0,
            group_stack: Vec::new(),
            anchors: HashMap::default(),
            gap_metadata: HashMap::default(),
            anchors_dirty: false,
            next_anchor_id: Cell::new(1), // Start at 1 (0 is INVALID)
            free_anchor_ids: Vec::new(),
            last_start_was_gap: false,
            orphaned_node_ids: OrphanedNodeIds::default(),
            needs_compact: false,
        }
    }

    fn replace_slot_tracked(&mut self, index: usize, slot: Slot) -> Slot {
        self.clear_gap_metadata_for_replacement(index, &slot);
        std::mem::replace(&mut self.slots[index], slot)
    }

    fn set_slot_tracked(&mut self, index: usize, slot: Slot) {
        self.clear_gap_metadata_for_replacement(index, &slot);
        self.slots[index] = slot;
    }

    fn push_slot_tracked(&mut self, slot: Slot) {
        self.slots.push(slot);
    }

    /// Returns approximate heap bytes used by this slot table.
    pub fn heap_bytes(&self) -> usize {
        let slot_size = std::mem::size_of::<Slot>();
        let slots_bytes = self.slots.capacity() * slot_size;
        let pending_bytes = self.pending_slot_drops.capacity() * slot_size;
        let anchors_bytes = self.anchors.capacity() * std::mem::size_of::<(usize, usize)>();
        let gap_metadata_bytes =
            self.gap_metadata.capacity() * std::mem::size_of::<(usize, GapMetadata)>();
        let free_anchors_bytes = self.free_anchor_ids.capacity() * std::mem::size_of::<usize>();
        let group_stack_bytes = self.group_stack.capacity() * std::mem::size_of::<GroupFrame>();
        let orphaned_node_ids_bytes =
            self.orphaned_node_ids.capacity() * std::mem::size_of::<OrphanedNode>();
        slots_bytes
            + pending_bytes
            + anchors_bytes
            + gap_metadata_bytes
            + free_anchors_bytes
            + group_stack_bytes
            + orphaned_node_ids_bytes
    }

    pub fn debug_stats(&self) -> SlotTableDebugStats {
        SlotTableDebugStats {
            slots_len: self.slots.len(),
            slots_cap: self.slots.capacity(),
            pending_slot_drops_len: self.pending_slot_drops.len(),
            pending_slot_drops_cap: self.pending_slot_drops.capacity(),
            anchors_len: self.anchors.len(),
            anchors_cap: self.anchors.capacity(),
            free_anchor_ids_len: self.free_anchor_ids.len(),
            free_anchor_ids_cap: self.free_anchor_ids.capacity(),
            group_stack_len: self.group_stack.len(),
            group_stack_cap: self.group_stack.capacity(),
            orphaned_node_ids_len: self.orphaned_node_ids.len(),
            orphaned_node_ids_cap: self.orphaned_node_ids.capacity(),
        }
    }

    pub fn current_group(&self) -> usize {
        self.group_stack.last().map(|f| f.start).unwrap_or(0)
    }

    fn clear_gap_metadata_for_replacement(&mut self, index: usize, new_slot: &Slot) {
        if matches!(new_slot, Slot::Gap { .. }) {
            return;
        }
        let old_anchor = match self.slots.get(index) {
            Some(Slot::Gap { anchor }) => Some(*anchor),
            _ => None,
        };
        if let Some(anchor) = old_anchor {
            self.clear_gap_metadata(anchor);
        }
    }

    fn gap_metadata_for_anchor(&self, anchor: AnchorId) -> Option<GapMetadata> {
        anchor
            .is_valid()
            .then(|| self.gap_metadata.get(&anchor.0).copied())
            .flatten()
    }

    fn set_gap_metadata(&mut self, anchor: AnchorId, metadata: GapMetadata) {
        if !anchor.is_valid() {
            return;
        }
        if metadata == GapMetadata::default() {
            self.gap_metadata.remove(&anchor.0);
        } else {
            self.gap_metadata.insert(anchor.0, metadata);
        }
    }

    fn clear_gap_metadata(&mut self, anchor: AnchorId) {
        if anchor.is_valid() {
            self.gap_metadata.remove(&anchor.0);
        }
    }

    fn gap_group_key(&self, anchor: AnchorId) -> Option<Key> {
        self.gap_metadata_for_anchor(anchor)
            .and_then(|metadata| metadata.group_key)
    }

    fn gap_boundary_key(&self, anchor: AnchorId) -> Option<Key> {
        self.gap_metadata_for_anchor(anchor)
            .and_then(|metadata| metadata.boundary_key)
    }

    fn gap_group_scope(&self, anchor: AnchorId) -> PackedScopeId {
        self.gap_metadata_for_anchor(anchor)
            .map(|metadata| metadata.group_scope)
            .unwrap_or_default()
    }

    fn gap_group_len(&self, anchor: AnchorId) -> u32 {
        self.gap_metadata_for_anchor(anchor)
            .map(|metadata| metadata.group_len)
            .unwrap_or(0)
    }

    fn gap_preserved_node(&self, anchor: AnchorId) -> Option<(NodeId, u32)> {
        self.gap_metadata_for_anchor(anchor)
            .and_then(|metadata| metadata.preserved_node)
    }

    fn gap_extent_for_anchor(&self, anchor: AnchorId) -> usize {
        unpack_slot_len(self.gap_group_len(anchor)).max(1)
    }

    fn current_disallow_live_slot_reuse(&self) -> bool {
        self.group_stack
            .last()
            .map(|frame| {
                frame.fresh_body && matches!(frame.child_reuse, ChildReusePolicy::FreshInsert)
            })
            .unwrap_or(false)
    }

    fn current_parent_allows_exact_gap_node_reuse(&self) -> bool {
        self.group_stack
            .last()
            .map(|frame| frame.child_reuse.allows_exact_live_reuse())
            .unwrap_or(true)
    }

    fn inherited_gap_boundary_key(&self) -> Option<Key> {
        self.group_stack.last().and_then(|frame| {
            if frame.child_reuse.requires_restricted_reuse() {
                Some(frame.gap_boundary_key)
            } else {
                None
            }
        })
    }

    fn next_gap_boundary_key(&self, key: Key, child_reuse: ChildReusePolicy) -> Key {
        if child_reuse.requires_restricted_reuse() {
            self.inherited_gap_boundary_key().unwrap_or(key)
        } else {
            key
        }
    }

    fn current_parent_gap_boundary_key(&self) -> Option<Key> {
        self.group_stack.last().map(|frame| frame.gap_boundary_key)
    }

    fn gap_matches_current_boundary(&self, boundary_key: Option<Key>) -> bool {
        match (self.current_parent_gap_boundary_key(), boundary_key) {
            (Some(current), Some(boundary)) => current == boundary,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn owner_gap_boundary_key(&self, owner_index: Option<usize>) -> Option<Key> {
        let owner_index = owner_index?;
        if let Some(frame) = self
            .group_stack
            .iter()
            .rev()
            .find(|frame| frame.start == owner_index)
        {
            return Some(frame.gap_boundary_key);
        }

        match self.slots.get(owner_index) {
            Some(Slot::Group { boundary_key, .. }) => Some(*boundary_key),
            Some(Slot::Gap { anchor }) => self
                .gap_boundary_key(*anchor)
                .or_else(|| self.gap_group_key(*anchor)),
            _ => None,
        }
    }

    pub fn group_key(&self, index: usize) -> Option<Key> {
        match self.slots.get(index) {
            Some(Slot::Group { key, .. }) => Some(*key),
            Some(Slot::Gap { anchor }) => self.gap_group_key(*anchor),
            _ => None,
        }
    }

    fn ensure_capacity(&mut self) {
        if self.slots.is_empty() {
            self.slots.reserve_exact(Self::INITIAL_CAP);
            self.append_gap_slots(Self::INITIAL_CAP);
        } else if self.cursor == self.slots.len() {
            self.grow_slots();
        }
    }

    fn force_gap_here(&mut self, cursor: usize) {
        // we *know* we have capacity (ensure_capacity() already ran)
        // so just overwrite the slot at cursor with a fresh gap
        self.replace_slot_deferred(
            cursor,
            Slot::Gap {
                anchor: AnchorId::INVALID,
            },
        );
    }

    fn find_right_gap_run(&self, from: usize, scan_limit: usize) -> Option<(usize, usize)> {
        let end = (from + scan_limit).min(self.slots.len());
        let mut i = from;
        while i < end {
            if let Some(Slot::Gap { anchor, .. }) = self.slots.get(i) {
                if *anchor == AnchorId::INVALID {
                    let start = i;
                    let mut len = 1;
                    while i + len < end {
                        match self.slots.get(i + len) {
                            Some(Slot::Gap { anchor, .. }) if *anchor == AnchorId::INVALID => {
                                len += 1;
                            }
                            _ => break,
                        }
                    }
                    return Some((start, len));
                }
            }
            i += 1;
        }
        None
    }

    fn find_tail_gap_run(&self) -> Option<(usize, usize)> {
        let mut run_len = 0usize;
        let mut run_start = 0usize;
        let mut found = false;

        for index in (0..self.slots.len()).rev() {
            match self.slots.get(index) {
                Some(Slot::Gap { anchor, .. }) if *anchor == AnchorId::INVALID => {
                    run_start = index;
                    run_len += 1;
                    found = true;
                }
                _ if found => break,
                _ => {}
            }
        }

        found.then_some((run_start, run_len))
    }

    fn ensure_gap_at_local(&mut self, cursor: usize) {
        if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
            return;
        }
        self.ensure_capacity();

        // Fast path: look for a gap run within the local scan window.
        if let Some((run_start, run_len)) = self.find_right_gap_run(cursor, Self::LOCAL_GAP_SCAN) {
            self.shift_group_frames(cursor, run_len as isize);
            self.shift_anchor_positions_from(cursor, run_len as isize);
            self.slots[cursor..run_start + run_len].rotate_right(run_len);
            return;
        }

        // Slow path: after compaction the nearest gap may be far away.
        // Search the entire tail before falling back to the destructive force_gap_here.
        let full_range = self.slots.len().saturating_sub(cursor);
        if full_range > Self::LOCAL_GAP_SCAN {
            if let Some((run_start, run_len)) = self.find_right_gap_run(cursor, full_range) {
                self.shift_group_frames(cursor, run_len as isize);
                self.shift_anchor_positions_from(cursor, run_len as isize);
                self.slots[cursor..run_start + run_len].rotate_right(run_len);
                return;
            }
        }

        // No gaps anywhere — grow the vec and try once more.
        self.grow_slots();
        let full_range = self.slots.len().saturating_sub(cursor);
        if let Some((run_start, run_len)) = self.find_right_gap_run(cursor, full_range) {
            self.shift_group_frames(cursor, run_len as isize);
            self.shift_anchor_positions_from(cursor, run_len as isize);
            self.slots[cursor..run_start + run_len].rotate_right(run_len);
            return;
        }

        self.force_gap_here(cursor);
    }

    fn preserve_terminal_group_block_at_tail(&mut self, start: usize, len: usize) {
        if len == 0 {
            return;
        }

        // Preserve the replaced block out-of-line without collapsing the original span.
        // Siblings to the right must keep their physical positions so partial recomposition
        // cannot inherit or trim them through a stale parent extent.
        let mut moved = Vec::with_capacity(len);
        for index in start..start + len {
            moved.push(self.replace_slot_tracked(
                index,
                Slot::Gap {
                    anchor: AnchorId::INVALID,
                },
            ));
        }
        self.anchors_dirty = true;

        loop {
            let (run_start, run_len) = self.find_tail_gap_run().unwrap_or((self.slots.len(), 0));
            if run_len >= len {
                let dest_start = run_start + run_len - len;
                if dest_start < start + len {
                    self.grow_slots();
                    continue;
                }
                for (offset, slot) in moved.drain(..).enumerate() {
                    self.set_slot_tracked(dest_start + offset, slot);
                }
                return;
            }

            self.grow_slots();
        }
    }

    fn append_gap_slots(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        for _ in 0..count {
            self.slots.push(Slot::Gap {
                anchor: AnchorId::INVALID,
            });
        }
    }

    fn grow_slots(&mut self) {
        let old_len = self.slots.len();
        let target_len = Self::next_slot_target_len(old_len);
        let additional = target_len.saturating_sub(old_len);
        if additional == 0 {
            return;
        }
        self.slots.reserve_exact(additional);
        self.append_gap_slots(additional);
    }

    fn next_slot_target_len(old_len: usize) -> usize {
        if old_len < Self::INITIAL_CAP {
            return Self::INITIAL_CAP;
        }
        if old_len < Self::LARGE_GROWTH_THRESHOLD {
            return old_len.saturating_mul(2);
        }

        let incremental_growth = (old_len / Self::LARGE_GROWTH_DIVISOR).max(Self::INITIAL_CAP);
        old_len.saturating_add(incremental_growth)
    }

    /// Allocate a new unique anchor ID.
    fn allocate_anchor(&mut self) -> AnchorId {
        let id = self.next_anchor_id.get();
        self.next_anchor_id.set(id + 1);
        AnchorId(id)
    }

    fn free_anchor(&mut self, anchor: AnchorId) {
        if anchor.is_valid() && anchor.0 != 0 {
            self.anchors.remove(&anchor.0);
            self.gap_metadata.remove(&anchor.0);
        }
    }

    fn replace_slot_deferred(&mut self, index: usize, slot: Slot) {
        let old = self.replace_slot_tracked(index, slot);
        old.deactivate_scope();
        // Free the old slot's anchor if it differs from the new slot's anchor
        let old_anchor = old.anchor_id();
        let new_anchor = self.slots[index].anchor_id();
        if old_anchor != new_anchor {
            self.free_anchor(old_anchor);
        }
        self.pending_slot_drops.push(old);
    }

    fn replace_gap_slot_deferred(
        &mut self,
        index: usize,
        group_key: Option<Key>,
        boundary_key: Option<Key>,
        group_scope: PackedScopeId,
        group_len: u32,
    ) {
        let anchor = self.slots[index].anchor_id();
        let preserved_node = match self.slots.get(index) {
            Some(Slot::Node { id, gen, .. }) => Some((*id, *gen)),
            Some(Slot::Gap { anchor }) => self.gap_preserved_node(*anchor),
            _ => None,
        };
        let old = self.replace_slot_tracked(index, Slot::Gap { anchor });
        self.set_gap_metadata(
            anchor,
            GapMetadata {
                group_key,
                boundary_key,
                group_scope,
                group_len,
                preserved_node,
            },
        );
        old.deactivate_scope();
        if let Slot::Node { id, gen, .. } = old {
            self.orphaned_node_ids
                .push(OrphanedNode::new(id, gen, anchor));
        }
        self.pending_slot_drops.push(old);
    }

    fn flush_pending_slot_drops(&mut self) {
        self.pending_slot_drops.clear_and_drop_reverse();
        let retained = self
            .pending_slot_drops
            .len()
            .max(Self::MIN_RETAINED_PENDING_SLOT_DROPS_CAPACITY);
        self.pending_slot_drops.trim_retained_capacity(retained);
    }

    /// Register an anchor at a specific position in the slots array.
    fn register_anchor(&mut self, anchor: AnchorId, position: usize) {
        debug_assert!(anchor.is_valid(), "attempted to register invalid anchor");
        let idx = anchor.0;
        if idx == 0 {
            return;
        }
        self.anchors.insert(idx, position);
    }

    /// Returns whether the most recent `start` invocation reused a gap slot.
    /// Resets the flag to false after reading.
    fn take_last_start_was_gap(&mut self) -> bool {
        let was_gap = self.last_start_was_gap;
        self.last_start_was_gap = false;
        was_gap
    }

    /// Resolve an anchor to its current position in the slots array.
    fn resolve_anchor(&self, anchor: AnchorId) -> Option<usize> {
        let idx = anchor.0;
        if idx == 0 {
            return None;
        }
        self.anchors.get(&idx).copied()
    }

    /// Mark a range of slots as gaps instead of truncating.
    /// This preserves sibling components while allowing structure changes.
    /// When encountering a Group, recursively marks the entire group structure as gaps.
    pub fn mark_range_as_gaps(
        &mut self,
        start: usize,
        end: usize,
        owner_index: Option<usize>,
    ) -> bool {
        self.mark_range_as_gaps_impl(start, end, owner_index, true)
    }

    fn mark_range_as_gaps_impl(
        &mut self,
        start: usize,
        end: usize,
        owner_index: Option<usize>,
        preserve_group_metadata: bool,
    ) -> bool {
        let end = end.min(self.slots.len());
        let boundary_key = self.owner_gap_boundary_key(owner_index);
        let mut marked_any = false;

        if !preserve_group_metadata {
            for index in start..end {
                self.replace_gap_slot_deferred(
                    index,
                    None,
                    boundary_key,
                    PackedScopeId::default(),
                    0,
                );
                marked_any = true;
            }
            if marked_any {
                self.needs_compact = true;
                if let Some(idx) = owner_index {
                    if let Some(Slot::Group {
                        has_gap_children, ..
                    }) = self.slots.get_mut(idx)
                    {
                        *has_gap_children = true;
                    }
                }
            }
            return marked_any;
        }

        let mut i = start;

        while i < end {
            if i >= self.slots.len() {
                break;
            }

            let (group_len, group_key, group_scope) = {
                let slot = &self.slots[i];
                let (group_len, group_key, group_scope) = match slot {
                    Slot::Group {
                        len, key, scope, ..
                    } if preserve_group_metadata => (*len, Some(*key), *scope),
                    Slot::Group { len, .. } => (*len, None, PackedScopeId::default()),
                    Slot::Gap { anchor } if preserve_group_metadata => (
                        self.gap_group_len(*anchor),
                        self.gap_group_key(*anchor),
                        self.gap_group_scope(*anchor),
                    ),
                    Slot::Gap { anchor } => {
                        (self.gap_group_len(*anchor), None, PackedScopeId::default())
                    }
                    _ => (0, None, PackedScopeId::default()),
                };
                (group_len, group_key, group_scope)
            };

            // Mark this slot as a gap, preserving Group metadata if it was a Group
            // This allows Groups to be properly matched and reused during tab switching
            self.replace_gap_slot_deferred(i, group_key, boundary_key, group_scope, group_len);
            marked_any = true;

            // If it was a group, recursively mark its children as gaps too
            if group_len > 0 {
                // Mark children (from i+1 to i+group_len)
                let children_end = (i + unpack_slot_len(group_len)).min(end);
                for j in (i + 1)..children_end {
                    if j < self.slots.len() {
                        match &self.slots[j] {
                            Slot::Group {
                                key, scope, len, ..
                            } if preserve_group_metadata => {
                                self.replace_gap_slot_deferred(
                                    j,
                                    Some(*key),
                                    boundary_key,
                                    *scope,
                                    *len,
                                );
                                marked_any = true;
                            }
                            Slot::Group { .. } => {
                                self.replace_gap_slot_deferred(
                                    j,
                                    None,
                                    boundary_key,
                                    PackedScopeId::default(),
                                    0,
                                );
                                marked_any = true;
                            }
                            Slot::Gap { anchor } if preserve_group_metadata => {
                                self.replace_gap_slot_deferred(
                                    j,
                                    self.gap_group_key(*anchor),
                                    boundary_key,
                                    self.gap_group_scope(*anchor),
                                    self.gap_group_len(*anchor),
                                );
                                marked_any = true;
                            }
                            Slot::Gap { .. } => {
                                self.replace_gap_slot_deferred(
                                    j,
                                    None,
                                    boundary_key,
                                    PackedScopeId::default(),
                                    0,
                                );
                                marked_any = true;
                            }
                            Slot::Node { .. } => {
                                self.replace_gap_slot_deferred(
                                    j,
                                    None,
                                    boundary_key,
                                    PackedScopeId::default(),
                                    0,
                                );
                                marked_any = true;
                            }
                            Slot::ScopeValue { .. } => {
                                self.slots[j].deactivate_scope();
                            }
                            Slot::Value { .. } => {}
                        }
                    }
                }
                i = (i + unpack_slot_len(group_len)).max(i + 1);
            } else {
                i += 1;
            }
        }

        if marked_any {
            self.needs_compact = true;
            if let Some(idx) = owner_index {
                if let Some(Slot::Group {
                    has_gap_children, ..
                }) = self.slots.get_mut(idx)
                {
                    *has_gap_children = true;
                }
            }
        }
        marked_any
    }

    pub fn get_group_scope(&self, index: usize) -> Option<ScopeId> {
        let slot = self
            .slots
            .get(index)
            .expect("get_group_scope: index out of bounds");
        match slot {
            Slot::Group { scope, .. } => scope.to_option(),
            _ => None,
        }
    }

    pub fn set_group_scope(&mut self, index: usize, scope: ScopeId) {
        let slot = self
            .slots
            .get_mut(index)
            .expect("set_group_scope: index out of bounds");
        match slot {
            Slot::Group {
                scope: scope_opt, ..
            } => {
                // With gaps implementation, Groups can be reused across compositions.
                // Always update the scope to the current value.
                *scope_opt = PackedScopeId::new(Some(scope));
            }
            _ => panic!("set_group_scope: slot at index is not a group"),
        }
    }

    pub fn start_recranpose_at_anchor(
        &mut self,
        anchor: AnchorId,
        owner: ScopeId,
    ) -> Option<usize> {
        let index = self.resolve_anchor(anchor)?;
        match self.slots.get(index) {
            Some(Slot::Group { scope, .. }) if scope.to_option() == Some(owner) => {
                self.start_recompose(index);
                Some(index)
            }
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn find_group_index_by_scope(&self, scope: ScopeId) -> Option<usize> {
        self.slots
            .iter()
            .enumerate()
            .find_map(|(i, slot)| match slot {
                Slot::Group { scope: id, .. } if id.to_option() == Some(scope) => Some(i),
                _ => None,
            })
    }

    #[cfg(test)]
    pub fn start_recranpose_at_scope(&mut self, scope: ScopeId) -> Option<usize> {
        let index = self.find_group_index_by_scope(scope)?;
        self.start_recompose(index);
        Some(index)
    }

    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                Slot::Group {
                    key, len, scope, ..
                } => Some((i, *key, scope.to_option(), unpack_slot_len(*len))),
                _ => None,
            })
            .collect()
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        self.slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let kind = match slot {
                    Slot::Group {
                        key, scope, len, ..
                    } => format!(
                        "Group(key={:?}, scope={:?}, len={})",
                        key,
                        scope.to_option(),
                        unpack_slot_len(*len)
                    ),
                    Slot::Value { .. } => "Value".to_string(),
                    Slot::ScopeValue { .. } => "ScopeValue".to_string(),
                    Slot::Node { id, .. } => format!("Node(id={:?})", id),
                    Slot::Gap { anchor } => {
                        if let Some(key) = self.gap_group_key(*anchor) {
                            format!(
                                "Gap(was_group_key={:?}, scope={:?})",
                                key,
                                self.gap_group_scope(*anchor)
                            )
                        } else if let Some((id, gen)) = self.gap_preserved_node(*anchor) {
                            format!("Gap(was_node_id={id:?}, gen={gen})")
                        } else {
                            "Gap".to_string()
                        }
                    }
                };
                (i, kind)
            })
            .collect()
    }

    pub fn debug_value_type_counts(&self, limit: usize) -> Vec<SlotValueTypeDebugStat> {
        let mut counts: HashMap<&'static str, (usize, usize)> = HashMap::default();
        for slot in &self.slots {
            let Some((type_name, inline_payload_bytes)) = (match slot {
                Slot::Value { data, .. } => {
                    Some((data.debug_type_name(), data.debug_inline_payload_bytes()))
                }
                Slot::ScopeValue { .. } => Some((
                    std::any::type_name::<RecomposeScope>(),
                    std::mem::size_of::<RecomposeScope>(),
                )),
                _ => None,
            }) else {
                continue;
            };

            let entry = counts.entry(type_name).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += inline_payload_bytes;
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

    fn update_group_bounds(&mut self) {
        for frame in &mut self.group_stack {
            if frame.end < self.cursor {
                frame.end = self.cursor;
            }
        }
    }

    /// Update all anchor positions to match their current physical positions in the slots array.
    /// This should be called after any operation that modifies slot positions (insert, remove, etc.)
    fn rebuild_all_anchor_positions(&mut self) {
        let live_anchor_count = self
            .slots
            .iter()
            .filter(|slot| slot.anchor_id().is_valid())
            .count();
        let mut anchors = HashMap::default();
        anchors.reserve(live_anchor_count);
        for slot in &self.slots {
            let idx = slot.anchor_id().0;
            if idx != 0 {
                anchors.insert(idx, INVALID_ANCHOR_POS);
            }
        }

        for (position, slot) in self.slots.iter().enumerate() {
            let idx = slot.anchor_id().0;
            if idx == 0 {
                continue;
            }
            anchors.insert(idx, position);
        }
        self.anchors = anchors;
    }

    fn shift_group_frames(&mut self, index: usize, delta: isize) {
        if delta == 0 {
            return;
        }
        if delta > 0 {
            let delta = delta as usize;
            for frame in &mut self.group_stack {
                if frame.start >= index {
                    frame.start += delta;
                    frame.end += delta;
                } else if frame.end >= index {
                    frame.end += delta;
                }
            }
        } else {
            let delta = (-delta) as usize;
            for frame in &mut self.group_stack {
                if frame.start >= index {
                    frame.start = frame.start.saturating_sub(delta);
                    frame.end = frame.end.saturating_sub(delta);
                } else if frame.end > index {
                    frame.end = frame.end.saturating_sub(delta);
                }
            }
        }
    }

    pub fn start(&mut self, key: Key) -> usize {
        self.ensure_capacity();

        let cursor = self.cursor;
        let parent_reuse = self
            .group_stack
            .last()
            .map(|frame| frame.child_reuse)
            .unwrap_or(ChildReusePolicy::Normal);

        // === FAST PATH =======================================================
        if let Some(Slot::Group {
            key: existing_key,
            len,
            boundary_key,
            has_gap_children,
            ..
        }) = self.slots.get(cursor)
        {
            // Only fast-path if:
            // 1) key matches
            // 2) there were NO gap children before
            // 3) parent allows exact live-group reuse at the current cursor
            if *existing_key == key && !*has_gap_children && parent_reuse.allows_exact_live_reuse()
            {
                let inherited_fresh_body = !matches!(parent_reuse, ChildReusePolicy::Normal);
                self.last_start_was_gap = inherited_fresh_body;

                let frame = GroupFrame {
                    key,
                    start: cursor,
                    end: cursor + unpack_slot_len(*len),
                    child_reuse: parent_reuse,
                    fresh_body: inherited_fresh_body,
                    gap_boundary_key: *boundary_key,
                };
                self.group_stack.push(frame);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }
        }

        let restricted_reuse = parent_reuse.requires_restricted_reuse();
        let allow_exact_live_reuse = parent_reuse.allows_exact_live_reuse();
        let allow_live_search = matches!(parent_reuse, ChildReusePolicy::Normal);

        self.last_start_was_gap = false;
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );

        if cursor == self.slots.len() {
            self.grow_slots();
        }

        debug_assert!(
            cursor < self.slots.len(),
            "slot cursor {} failed to grow",
            cursor
        );

        if cursor > 0 && !matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
            if let Some(Slot::Group { key: prev_key, .. }) = self.slots.get(cursor - 1) {
                if *prev_key == key {
                    return self.insert_new_group_at_cursor(key, ChildReusePolicy::FreshInsert);
                }
            }
        }

        // Check if we can reuse an existing Group at the cursor position without scanning.
        if let Some(slot) = self.slots.get_mut(cursor) {
            if let Slot::Group {
                key: existing_key,
                len,
                boundary_key,
                has_gap_children,
                ..
            } = slot
            {
                if *existing_key == key && allow_exact_live_reuse {
                    let group_len = *len;
                    let had_gap_children = *has_gap_children;
                    let gap_boundary_key = *boundary_key;
                    if had_gap_children {
                        *has_gap_children = false;
                    }

                    let inherited_fresh_body = !matches!(parent_reuse, ChildReusePolicy::Normal);
                    self.last_start_was_gap = inherited_fresh_body;
                    let frame = GroupFrame {
                        key,
                        start: cursor,
                        end: cursor + unpack_slot_len(group_len),
                        child_reuse: parent_reuse,
                        fresh_body: inherited_fresh_body,
                        gap_boundary_key,
                    };
                    self.group_stack.push(frame);
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
                    return cursor;
                }
            }
        }

        if let Some(Slot::Group {
            key: existing_key,
            anchor: _old_anchor,
            len: old_len,
            scope: old_scope,
            boundary_key: old_boundary_key,
            has_gap_children: _,
        }) = self.slots.get(cursor)
        {
            if *existing_key != key {
                let old_key = *existing_key;
                let old_len = unpack_slot_len(*old_len);
                let old_scope = old_scope.to_option();
                let old_boundary_key = *old_boundary_key;
                let parent_end = self
                    .group_stack
                    .last()
                    .map(|frame| frame.end.min(self.slots.len()))
                    .unwrap_or(self.slots.len());
                let preserve_at_tail = cursor + old_len == parent_end;

                if old_len > 1 {
                    let start = cursor + 1;
                    let end = cursor + old_len;
                    let _ = self.mark_range_as_gaps_impl(start, end, Some(cursor), false);
                }

                self.replace_gap_slot_deferred(
                    cursor,
                    Some(old_key),
                    Some(old_boundary_key),
                    PackedScopeId::new(old_scope),
                    pack_slot_len(old_len),
                );

                if preserve_at_tail {
                    self.preserve_terminal_group_block_at_tail(cursor, old_len);
                }
            }
        }

        // Check if we can reuse an existing gap in place.
        let reusable_gap = match self.slots.get(cursor) {
            Some(Slot::Gap { anchor }) => self.gap_group_key(*anchor).map(|gap_key| {
                (
                    *anchor,
                    gap_key,
                    self.gap_boundary_key(*anchor),
                    self.gap_group_scope(*anchor),
                    self.gap_group_len(*anchor),
                )
            }),
            _ => None,
        };
        let parent_end = self
            .group_stack
            .last()
            .map(|frame| frame.end.min(self.slots.len()))
            .unwrap_or(self.slots.len());

        if let Some((existing_anchor, gap_key, boundary_key, group_scope, group_len)) = reusable_gap
        {
            if gap_key == key
                && (self.gap_matches_current_boundary(boundary_key) || allow_live_search)
            {
                let mut search_index = cursor.saturating_add(unpack_slot_len(group_len).max(1));
                while search_index < parent_end {
                    match self.slots.get(search_index) {
                        Some(Slot::Group {
                            key: existing_key, ..
                        }) if *existing_key == key => {
                            let group_len = match self.slots.get(search_index) {
                                Some(Slot::Group { len, .. }) => unpack_slot_len(*len),
                                _ => unreachable!("matched group must still be present"),
                            };
                            self.last_start_was_gap = false;
                            let actual_len = group_len
                                .max(1)
                                .min(self.slots.len().saturating_sub(search_index));
                            self.shift_group_frames(search_index, -(actual_len as isize));
                            let moved: Vec<_> = self
                                .slots
                                .drain(search_index..search_index + actual_len)
                                .collect();
                            self.shift_group_frames(cursor, moved.len() as isize);
                            self.slots.splice(cursor..cursor, moved);
                            self.anchors_dirty = true;
                            let gap_boundary_key = boundary_key
                                .unwrap_or_else(|| self.next_gap_boundary_key(key, parent_reuse));
                            let frame = GroupFrame {
                                key,
                                start: cursor,
                                end: cursor + actual_len,
                                child_reuse: ChildReusePolicy::Normal,
                                fresh_body: false,
                                gap_boundary_key,
                            };
                            self.group_stack.push(frame);
                            self.cursor = cursor + 1;
                            self.update_group_bounds();
                            return cursor;
                        }
                        Some(Slot::Group { len, .. }) => {
                            search_index =
                                search_index.saturating_add(unpack_slot_len(*len).max(1));
                        }
                        Some(Slot::Gap { anchor }) => {
                            search_index =
                                search_index.saturating_add(self.gap_extent_for_anchor(*anchor));
                        }
                        Some(_) => search_index += 1,
                        None => break,
                    }
                }

                let len = unpack_slot_len(group_len);
                let group_anchor = if existing_anchor.is_valid() {
                    existing_anchor
                } else {
                    self.allocate_anchor()
                };
                let preserved_scope = group_scope.to_option();
                let gap_boundary_key = if matches!(parent_reuse, ChildReusePolicy::FreshInsert) {
                    self.next_gap_boundary_key(key, parent_reuse)
                } else {
                    boundary_key.unwrap_or_else(|| self.next_gap_boundary_key(key, parent_reuse))
                };
                let child_reuse = if matches!(parent_reuse, ChildReusePolicy::FreshInsert) {
                    ChildReusePolicy::FreshInsert
                } else {
                    ChildReusePolicy::ParentRestoredFromGap
                };

                self.set_slot_tracked(
                    cursor,
                    Slot::Group {
                        key,
                        anchor: group_anchor,
                        len: pack_slot_len(len),
                        scope: PackedScopeId::new(preserved_scope),
                        boundary_key: gap_boundary_key,
                        has_gap_children: false,
                    },
                );
                self.register_anchor(group_anchor, cursor);

                self.last_start_was_gap = true;
                let frame = GroupFrame {
                    key,
                    start: cursor,
                    end: cursor + len,
                    child_reuse,
                    fresh_body: true,
                    gap_boundary_key,
                };
                self.group_stack.push(frame);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }
        }

        let mut search_index = cursor;
        let mut found_group: Option<(usize, AnchorId, usize, Option<ScopeId>, Key, bool)> = None;
        const SEARCH_BUDGET: usize = 16;
        let search_budget = if restricted_reuse {
            parent_end.saturating_sub(cursor).max(SEARCH_BUDGET)
        } else {
            SEARCH_BUDGET
        };
        let mut scanned = 0usize;
        while search_index < parent_end && scanned < search_budget {
            scanned += 1;
            match self.slots.get(search_index) {
                Some(Slot::Group {
                    key: existing_key,
                    anchor,
                    len,
                    scope,
                    boundary_key,
                    ..
                }) => {
                    if *existing_key == key && allow_live_search {
                        found_group = Some((
                            search_index,
                            *anchor,
                            unpack_slot_len(*len),
                            scope.to_option(),
                            *boundary_key,
                            false,
                        ));
                        break;
                    }
                    search_index = search_index.saturating_add(unpack_slot_len(*len).max(1));
                }
                Some(Slot::Gap { anchor }) => {
                    if self.gap_group_key(*anchor) == Some(key)
                        && (self.gap_matches_current_boundary(self.gap_boundary_key(*anchor))
                            || allow_live_search)
                    {
                        found_group = Some((
                            search_index,
                            *anchor,
                            unpack_slot_len(self.gap_group_len(*anchor)),
                            self.gap_group_scope(*anchor).to_option(),
                            self.gap_boundary_key(*anchor)
                                .unwrap_or_else(|| self.next_gap_boundary_key(key, parent_reuse)),
                            true,
                        ));
                        break;
                    }
                    search_index = search_index.saturating_add(self.gap_extent_for_anchor(*anchor));
                }
                Some(_slot) => {
                    search_index += 1;
                }
                None => break,
            }
        }

        if let Some((
            found_index,
            group_anchor,
            group_len,
            preserved_scope,
            gap_boundary_key,
            reused_gap,
        )) = found_group
        {
            if reused_gap {
                self.set_slot_tracked(
                    found_index,
                    Slot::Group {
                        key,
                        anchor: group_anchor,
                        len: pack_slot_len(group_len),
                        scope: PackedScopeId::new(preserved_scope),
                        boundary_key: gap_boundary_key,
                        has_gap_children: false,
                    },
                );
            }

            let restored_from_preserved = reused_gap || found_index != cursor;
            let inherited_fresh_body = !matches!(parent_reuse, ChildReusePolicy::Normal);
            self.last_start_was_gap = restored_from_preserved || inherited_fresh_body;
            let group_extent = group_len.max(1);
            let available = self.slots.len().saturating_sub(found_index);
            let actual_len = group_extent.min(available);
            if actual_len > 0 {
                self.shift_group_frames(found_index, -(actual_len as isize));
                let moved: Vec<_> = self
                    .slots
                    .drain(found_index..found_index + actual_len)
                    .collect();
                self.shift_group_frames(cursor, moved.len() as isize);
                self.slots.splice(cursor..cursor, moved);
                self.anchors_dirty = true;
                let child_reuse = if restored_from_preserved {
                    restored_child_reuse(parent_reuse)
                } else {
                    parent_reuse
                };
                let frame_gap_boundary_key =
                    if reused_gap && matches!(parent_reuse, ChildReusePolicy::FreshInsert) {
                        self.next_gap_boundary_key(key, parent_reuse)
                    } else {
                        gap_boundary_key
                    };
                let frame = GroupFrame {
                    key,
                    start: cursor,
                    end: cursor + actual_len,
                    child_reuse,
                    fresh_body: restored_from_preserved || inherited_fresh_body,
                    gap_boundary_key: frame_gap_boundary_key,
                };
                self.group_stack.push(frame);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            } else {
                self.shift_group_frames(found_index, 0);
            }
        }

        self.insert_new_group_at_cursor(key, ChildReusePolicy::FreshInsert)
    }

    fn insert_new_group_at_cursor(&mut self, key: Key, child_reuse: ChildReusePolicy) -> usize {
        // make sure we have space at the tail for pulling gaps
        self.ensure_capacity();

        let cursor = self.cursor;
        self.ensure_gap_at_local(cursor);

        if cursor < self.slots.len() {
            debug_assert!(matches!(self.slots[cursor], Slot::Gap { .. }));
            if let Some(Slot::Gap { anchor: old_anchor }) = self.slots.get(cursor) {
                self.free_anchor(*old_anchor);
            }
            let group_anchor = self.allocate_anchor();
            self.set_slot_tracked(
                cursor,
                Slot::Group {
                    key,
                    anchor: group_anchor,
                    len: 0,
                    scope: PackedScopeId::default(),
                    boundary_key: self.next_gap_boundary_key(key, child_reuse),
                    has_gap_children: false,
                },
            );
            self.register_anchor(group_anchor, cursor);
        } else {
            let group_anchor = self.allocate_anchor();
            self.push_slot_tracked(Slot::Group {
                key,
                anchor: group_anchor,
                len: 0,
                scope: PackedScopeId::default(),
                boundary_key: self.next_gap_boundary_key(key, child_reuse),
                has_gap_children: false,
            });
            self.register_anchor(group_anchor, cursor);
        }
        self.last_start_was_gap = false;
        self.cursor = cursor + 1;
        self.group_stack.push(GroupFrame {
            key,
            start: cursor,
            end: self.cursor,
            child_reuse,
            fresh_body: true,
            gap_boundary_key: self.next_gap_boundary_key(key, child_reuse),
        });
        self.update_group_bounds();
        cursor
    }
    fn shift_anchor_positions_from(&mut self, start_slot: usize, delta: isize) {
        for pos in self.anchors.values_mut() {
            if *pos >= start_slot {
                *pos = (*pos as isize + delta) as usize;
            }
        }
    }
    fn flush_anchors_if_dirty(&mut self) {
        if self.anchors_dirty {
            self.anchors_dirty = false;
            self.rebuild_all_anchor_positions();
        }
    }
    pub fn end(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            let end = self.cursor;
            let mut grew = false;
            if let Some(slot) = self.slots.get_mut(frame.start) {
                debug_assert_eq!(
                    SlotKind::Group,
                    slot.kind(),
                    "slot kind mismatch at {}",
                    frame.start
                );
                if let Slot::Group {
                    key,
                    len,
                    has_gap_children,
                    ..
                } = slot
                {
                    debug_assert_eq!(*key, frame.key, "group key mismatch");
                    // Calculate new length based on cursor position
                    let new_len = end.saturating_sub(frame.start);
                    let old_len = unpack_slot_len(*len);
                    if new_len < old_len {
                        *has_gap_children = true;
                    }
                    const SHRINK_MIN_DROP: usize = 64;
                    const SHRINK_RATIO: usize = 4;
                    if old_len > new_len
                        && old_len >= new_len.saturating_mul(SHRINK_RATIO)
                        && (old_len - new_len) >= SHRINK_MIN_DROP
                    {
                        *len = pack_slot_len(new_len);
                    } else {
                        grew = new_len > old_len;
                        *len = pack_slot_len(old_len.max(new_len));
                    }
                }
            }
            if grew {
                self.propagate_group_growth(frame.start, end);
            }
            if let Some(parent) = self.group_stack.last_mut() {
                if parent.end < end {
                    parent.end = end;
                }
            }
        }
    }

    fn start_recompose(&mut self, index: usize) {
        if let Some(slot) = self.slots.get(index) {
            debug_assert_eq!(
                SlotKind::Group,
                slot.kind(),
                "slot kind mismatch at {}",
                index
            );
            if let Slot::Group {
                key,
                len,
                boundary_key,
                ..
            } = *slot
            {
                let frame = GroupFrame {
                    key,
                    start: index,
                    end: index + unpack_slot_len(len),
                    child_reuse: ChildReusePolicy::Normal,
                    fresh_body: false,
                    gap_boundary_key: boundary_key,
                };
                self.group_stack.push(frame);
                self.cursor = index + 1;
                if self.cursor < self.slots.len()
                    && matches!(
                        self.slots.get(self.cursor),
                        Some(Slot::Value { .. } | Slot::ScopeValue { .. })
                    )
                {
                    self.cursor += 1;
                }
            }
        }
    }

    fn end_recompose(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            let actual_end = self.cursor;
            if actual_end < frame.end {
                let _ = self.mark_range_as_gaps(actual_end, frame.end, Some(frame.start));
                self.flush_pending_slot_drops();
            }
            // When a scope grows during recomposition (e.g. depth increase),
            // the group's stored `len` may be smaller than the actual extent.
            // Update it and propagate the growth to all ancestor groups so
            // that future gap-marking covers the full extent.
            if let Some(Slot::Group { len, .. }) = self.slots.get_mut(frame.start) {
                let actual_len = actual_end.saturating_sub(frame.start);
                if actual_len > unpack_slot_len(*len) {
                    *len = pack_slot_len(actual_len);
                    self.propagate_group_growth(frame.start, actual_end);
                }
            }
            self.cursor = actual_end;
        }
    }

    /// Walk the slot table from the root to `child_start`, updating the `len`
    /// of every ancestor Group that contains `child_start` but whose stored
    /// extent doesn't reach `new_end`.
    fn propagate_group_growth(&mut self, child_start: usize, new_end: usize) {
        let mut i = 0;
        while i < child_start {
            if let Some(Slot::Gap { anchor }) = self.slots.get(i) {
                i += self.gap_extent_for_anchor(*anchor);
                continue;
            }
            match self.slots.get_mut(i) {
                Some(Slot::Group { len, .. }) => {
                    let group_end = i + unpack_slot_len(*len);
                    if group_end > child_start {
                        // This group contains child_start — grow it if needed.
                        if group_end < new_end {
                            *len = pack_slot_len(new_end.saturating_sub(i));
                        }
                        // Descend into this group to find deeper ancestors.
                        i += 1;
                    } else {
                        // Doesn't contain child_start — skip.
                        i += unpack_slot_len(*len).max(1);
                    }
                }
                _ => {
                    i += 1;
                }
            }
        }
    }

    pub fn skip_current(&mut self) {
        if let Some(frame) = self.group_stack.last() {
            self.cursor = frame.end.min(self.slots.len());
        }
    }

    pub fn node_ids_in_current_group(&self) -> Vec<NodeId> {
        let Some(frame) = self.group_stack.last() else {
            return Vec::new();
        };
        let end = frame.end.min(self.slots.len());
        self.slots[frame.start..end]
            .iter()
            .filter_map(|slot| match slot {
                Slot::Node { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }

    pub fn descendant_scopes_in_current_group(
        &self,
        current_scope: ScopeId,
    ) -> Vec<RecomposeScope> {
        let Some(frame) = self.group_stack.last() else {
            return Vec::new();
        };

        let end = frame.end.min(self.slots.len());
        let mut scopes = Vec::new();
        let mut seen = HashMap::default();

        for slot in &self.slots[frame.start.saturating_add(1)..end] {
            let Slot::ScopeValue { scope, .. } = slot else {
                continue;
            };

            if scope.id() == current_scope || seen.insert(scope.id(), ()).is_some() {
                continue;
            }

            scopes.push(scope.clone());
        }

        scopes
    }

    pub fn use_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> usize {
        self.ensure_capacity();

        let cursor = self.cursor;
        let disallow_live_reuse = self.current_disallow_live_slot_reuse();
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );

        if cursor < self.slots.len() {
            let reusable_live_value_slot = !disallow_live_reuse
                && (matches!(
                    self.slots.get(cursor),
                    Some(Slot::Value { data, .. }) if data.as_any().is::<T>()
                ) || (TypeId::of::<T>() == TypeId::of::<RecomposeScope>()
                    && matches!(self.slots.get(cursor), Some(Slot::ScopeValue { .. }))));
            let allow_live_reuse = !disallow_live_reuse;
            if !allow_live_reuse && !matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                self.ensure_gap_at_local(cursor);
            }

            // Check if we can reuse the existing slot
            if reusable_live_value_slot {
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }

            // Check if the slot is a Gap that we can replace
            if let Some(Slot::Gap {
                anchor: old_anchor, ..
            }) = self.slots.get(cursor)
            {
                let old_anchor = *old_anchor;
                self.free_anchor(old_anchor);
                let anchor = self.allocate_anchor();
                self.set_slot_tracked(cursor, Self::make_value_slot(anchor, init()));
                self.register_anchor(anchor, cursor);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }

            // Type mismatch: replace current slot (mark old content as unreachable via gap)
            // We replace in-place to maintain cursor position
            let anchor = self.allocate_anchor();
            self.replace_slot_deferred(cursor, Self::make_value_slot(anchor, init()));
            self.register_anchor(anchor, cursor);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return cursor;
        }

        // We're at the end of the slot table, append new slot
        let anchor = self.allocate_anchor();
        let slot = Self::make_value_slot(anchor, init());
        self.push_slot_tracked(slot);
        self.register_anchor(anchor, cursor);
        self.cursor = cursor + 1;
        self.update_group_bounds();
        cursor
    }

    pub fn read_value<T: 'static>(&self, idx: usize) -> &T {
        let slot = self
            .slots
            .get(idx)
            .unwrap_or_else(|| panic!("slot index {} out of bounds", idx));
        debug_assert_eq!(
            SlotKind::Value,
            slot.kind(),
            "slot kind mismatch at {}",
            idx
        );
        slot.as_value()
    }

    pub fn read_value_mut<T: 'static>(&mut self, idx: usize) -> &mut T {
        let slot = self
            .slots
            .get_mut(idx)
            .unwrap_or_else(|| panic!("slot index {} out of bounds", idx));
        debug_assert_eq!(
            SlotKind::Value,
            slot.kind(),
            "slot kind mismatch at {}",
            idx
        );
        slot.as_value_mut()
    }

    pub fn write_value<T: 'static>(&mut self, idx: usize, value: T) {
        if idx >= self.slots.len() {
            panic!("attempted to write slot {} out of bounds", idx);
        }
        let slot = &mut self.slots[idx];
        debug_assert_eq!(
            SlotKind::Value,
            slot.kind(),
            "slot kind mismatch at {}",
            idx
        );
        // Preserve the anchor when replacing the value
        let anchor = slot.anchor_id();
        *slot = Self::make_value_slot(anchor, value);
    }

    /// Read a value slot by its anchor ID.
    /// Provides stable access even if the slot's position changes.
    pub fn read_value_by_anchor<T: 'static>(&self, anchor: AnchorId) -> Option<&T> {
        let idx = self.resolve_anchor(anchor)?;
        Some(self.read_value(idx))
    }

    /// Read a mutable value slot by its anchor ID.
    pub fn read_value_mut_by_anchor<T: 'static>(&mut self, anchor: AnchorId) -> Option<&mut T> {
        let idx = self.resolve_anchor(anchor)?;
        Some(self.read_value_mut(idx))
    }

    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let index = self.use_value_slot(|| Owned::new(init()));
        self.read_value::<Owned<T>>(index).clone()
    }

    /// Remember a value and return both its index and anchor ID.
    /// The anchor provides stable access even if the slot's position changes.
    pub fn remember_with_anchor<T: 'static>(
        &mut self,
        init: impl FnOnce() -> T,
    ) -> (usize, AnchorId) {
        let index = self.use_value_slot(|| Owned::new(init()));
        let anchor = self
            .slots
            .get(index)
            .map(|slot| slot.anchor_id())
            .unwrap_or(AnchorId::INVALID);
        (index, anchor)
    }

    pub fn record_node(&mut self, id: NodeId, gen: u32) {
        self.ensure_capacity();

        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        if cursor < self.slots.len() {
            // Check if we can reuse the existing node slot
            if let Some(Slot::Node {
                id: existing,
                gen: existing_gen,
                ..
            }) = self.slots.get(cursor)
            {
                if *existing == id && *existing_gen == gen {
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
                    return;
                }
            }
            if let Some(Slot::Gap { anchor: old_anchor }) = self.slots.get(cursor) {
                let preserved = self.gap_preserved_node(*old_anchor);
                if self.current_parent_allows_exact_gap_node_reuse() && preserved == Some((id, gen))
                {
                    let anchor = if old_anchor.is_valid() {
                        *old_anchor
                    } else {
                        self.allocate_anchor()
                    };
                    self.set_slot_tracked(cursor, Slot::Node { anchor, id, gen });
                    self.register_anchor(anchor, cursor);
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
                    return;
                }
            }

            // Check if the slot is a Gap that we can replace
            if let Some(Slot::Gap {
                anchor: old_anchor, ..
            }) = self.slots.get(cursor)
            {
                let old_anchor = *old_anchor;
                self.free_anchor(old_anchor);
                let anchor = self.allocate_anchor();
                self.set_slot_tracked(cursor, Slot::Node { anchor, id, gen });
                self.register_anchor(anchor, cursor);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return;
            }

            // Type mismatch: Replace the slot directly with the new node
            let anchor = self.allocate_anchor();
            self.replace_slot_deferred(cursor, Slot::Node { anchor, id, gen });
            self.register_anchor(anchor, cursor);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return;
        }

        // No existing slot at cursor: add new slot
        let anchor = self.allocate_anchor();
        let slot = Slot::Node { anchor, id, gen };
        self.push_slot_tracked(slot);
        self.register_anchor(anchor, cursor);
        self.cursor = cursor + 1;
        self.update_group_bounds();
    }

    pub fn peek_node(&self) -> Option<(NodeId, u32)> {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        match self.slots.get(cursor) {
            Some(Slot::Node { id, gen, .. }) => Some((*id, *gen)),
            Some(Slot::Gap { anchor }) if self.current_parent_allows_exact_gap_node_reuse() => {
                self.gap_preserved_node(*anchor)
            }
            Some(_slot) => None,
            None => None,
        }
    }

    pub fn read_node(&mut self) -> Option<NodeId> {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        let node = match self.slots.get(cursor) {
            Some(Slot::Node { id, .. }) => Some(*id),
            Some(_slot) => None,
            None => None,
        };
        if node.is_some() {
            self.cursor = cursor + 1;
            self.update_group_bounds();
        }
        node
    }

    pub fn advance_after_node_read(&mut self) {
        let cursor = self.cursor;
        let preserved = match self.slots.get(cursor) {
            Some(Slot::Gap { anchor }) if self.current_parent_allows_exact_gap_node_reuse() => self
                .gap_preserved_node(*anchor)
                .map(|(id, gen)| (*anchor, id, gen)),
            _ => None,
        };
        if let Some((old_anchor, id, gen)) = preserved {
            let anchor = if old_anchor.is_valid() {
                old_anchor
            } else {
                self.allocate_anchor()
            };
            self.set_slot_tracked(cursor, Slot::Node { anchor, id, gen });
            self.register_anchor(anchor, cursor);
        }
        self.cursor += 1;
        self.update_group_bounds();
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
        self.group_stack.clear();
    }

    /// Step the cursor back by one position.
    /// Used when we need to replace a slot that was just read but turned out to be incompatible.
    pub fn step_back(&mut self) {
        debug_assert!(self.cursor > 0, "Cannot step back from cursor 0");
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Trim slots by marking unreachable slots as gaps.
    ///
    /// Instead of blindly truncating at cursor position, this method:
    /// 1. Marks slots from cursor to end of current group as gaps
    /// 2. Keeps the group length unchanged (gaps are part of the group's physical extent)
    /// 3. Preserves sibling components outside the current group
    ///
    /// This ensures effect states (LaunchedEffect, etc.) are preserved even when
    /// conditional rendering changes the composition structure.
    ///
    /// Key insight: Gap slots remain part of the group's physical length. The group's
    /// `len` field represents its physical extent in the slots array, not the count of
    /// active slots. This allows gap slots to be found and reused in subsequent compositions.
    pub fn trim_to_cursor(&mut self) -> bool {
        let mut marked = false;
        if let Some((owner_start, group_end)) = self
            .group_stack
            .last()
            .map(|frame| (frame.start, frame.end.min(self.slots.len())))
        {
            // Mark unreachable slots within this group as gaps
            if self.cursor < group_end
                && self.mark_range_as_gaps(self.cursor, group_end, Some(owner_start))
            {
                marked = true;
            }

            // Update the frame end to current cursor
            if let Some(frame) = self.group_stack.last_mut() {
                frame.end = self.cursor;
            }
        } else if self.cursor < self.slots.len() {
            // If there's no group stack, we're at the root level
            // Mark everything beyond cursor as gaps
            if self.mark_range_as_gaps(self.cursor, self.slots.len(), None) {
                marked = true;
            }
        }
        self.flush_pending_slot_drops();

        marked
    }

    /// Drain orphaned node IDs collected during gap marking.
    pub fn drain_orphaned_node_ids_with(&mut self, visitor: impl FnMut(OrphanedNode)) {
        self.orphaned_node_ids.drain_forward(visitor);
    }

    pub fn drain_orphaned_node_ids(&mut self) -> Vec<OrphanedNode> {
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
        match self.slots.get(index) {
            Some(Slot::Node { id, gen, .. })
                if *id == orphaned.id && *gen == orphaned.generation =>
            {
                NodeSlotState::Active
            }
            Some(Slot::Gap { anchor })
                if self.gap_preserved_node(*anchor) == Some((orphaned.id, orphaned.generation)) =>
            {
                NodeSlotState::PreservedGap
            }
            _ => NodeSlotState::Missing,
        }
    }

    #[cfg(test)]
    fn gap_metadata_at(&self, index: usize) -> Option<GapMetadata> {
        match self.slots.get(index) {
            Some(Slot::Gap { anchor }) => self.gap_metadata_for_anchor(*anchor),
            _ => None,
        }
    }

    #[cfg(test)]
    fn group_anchor_at(&self, index: usize) -> AnchorId {
        self.slots
            .get(index)
            .map(|slot| slot.anchor_id())
            .unwrap_or(AnchorId::INVALID)
    }

    #[cfg(test)]
    pub(crate) fn push_orphaned_node_for_test(&mut self, id: NodeId, generation: u32) {
        self.orphaned_node_ids
            .push(OrphanedNode::new(id, generation, AnchorId::INVALID));
    }

    /// Remove all Gap slots from the slot table, recalculate Group extents,
    /// and rebuild anchor positions. This reclaims memory that accumulated
    /// when groups shrank (e.g. recursive layout depth decrease, tab switching).
    ///
    /// Only runs when `needs_compact` was set by `mark_range_as_gaps`.
    pub fn compact(&mut self) {
        if !self.needs_compact {
            return;
        }

        let old_len = self.slots.len();
        if old_len == 0 {
            self.needs_compact = false;
            return;
        }

        // Count gaps — bail early if nothing to remove.
        let mut gap_count = 0usize;
        let mut gap_scan = 0usize;
        while gap_scan < old_len {
            match &self.slots[gap_scan] {
                Slot::Gap { anchor } => {
                    let extent = self
                        .gap_extent_for_anchor(*anchor)
                        .min(old_len.saturating_sub(gap_scan));
                    gap_count = gap_count.saturating_add(extent);
                    gap_scan = gap_scan.saturating_add(extent);
                }
                _ => gap_scan += 1,
            }
        }
        if gap_count == 0 {
            self.needs_compact = false;
            return;
        }
        let new_len = old_len - gap_count;
        if !Self::should_compact_gaps(old_len, gap_count, new_len) {
            return;
        }
        self.needs_compact = false;
        log::debug!(
            "compact: {} slots → {} (removing {} gaps, capacity {})",
            old_len,
            new_len,
            gap_count,
            self.slots.capacity()
        );

        // ── Phase 1: update Group::len using a depth-bounded extent stack ──
        let mut group_stack = Vec::<GroupCompactionFrame>::new();
        let mut new_len = 0usize;

        let mut i = 0usize;
        while i < old_len {
            while group_stack.last().is_some_and(|frame| frame.end <= i) {
                let frame = group_stack.pop().expect("group extent frame");
                if let Slot::Group {
                    len,
                    has_gap_children,
                    ..
                } = &mut self.slots[frame.index]
                {
                    *len = pack_slot_len(new_len.saturating_sub(frame.kept_before));
                    *has_gap_children = false;
                }
            }

            match &self.slots[i] {
                Slot::Gap { anchor } => {
                    let extent = self
                        .gap_extent_for_anchor(*anchor)
                        .min(old_len.saturating_sub(i));
                    i = i.saturating_add(extent);
                    continue;
                }
                Slot::Group { len, .. } => {
                    group_stack.push(GroupCompactionFrame {
                        index: i,
                        end: i.saturating_add(unpack_slot_len(*len).max(1)).min(old_len),
                        kept_before: new_len,
                    });
                }
                Slot::Value { .. } | Slot::ScopeValue { .. } | Slot::Node { .. } => {}
            }

            new_len += 1;
            i += 1;
        }

        while let Some(frame) = group_stack.pop() {
            if let Slot::Group {
                len,
                has_gap_children,
                ..
            } = &mut self.slots[frame.index]
            {
                *len = pack_slot_len(new_len.saturating_sub(frame.kept_before));
                *has_gap_children = false;
            }
        }

        // ── Phase 2: drop anchor positions of removed gaps ───────────
        //
        // Gap extents now live in side storage keyed by the gap's root
        // anchor. Keep that metadata intact until the removed gap block has
        // been collected in phase 3.
        let mut i = 0usize;
        while i < old_len {
            if let Slot::Gap { anchor } = &self.slots[i] {
                let extent = self
                    .gap_extent_for_anchor(*anchor)
                    .min(old_len.saturating_sub(i));
                for j in i..i + extent {
                    let anchor = self.slots[j].anchor_id();
                    if anchor.is_valid() {
                        self.anchors.remove(&anchor.0);
                    }
                }
                i = i.saturating_add(extent);
            } else {
                i += 1;
            }
        }

        // ── Phase 3: stable compaction with ordered teardown ────────
        //
        // Gap removal can drop remembered state owners and sibling cleanup
        // effects together. Preserving original slot order for the removed
        // block ensures reverse-drop teardown still runs cleanups before the
        // state owners they may read.
        let mut compacted = Vec::with_capacity(new_len);
        let mut removed = Vec::with_capacity(gap_count);
        let mut read = 0usize;
        while read < old_len {
            if let Slot::Gap { anchor } = &self.slots[read] {
                let extent = self
                    .gap_extent_for_anchor(*anchor)
                    .min(old_len.saturating_sub(read));
                for index in read..read + extent {
                    removed.push(std::mem::take(&mut self.slots[index]));
                }
                read = read.saturating_add(extent);
                continue;
            }
            compacted.push(std::mem::take(&mut self.slots[read]));
            read += 1;
        }
        debug_assert_eq!(compacted.len(), new_len);
        debug_assert_eq!(removed.len(), gap_count);
        self.slots = compacted;
        drop_slots_in_reverse(&mut removed);
        self.gap_metadata.clear();
        self.rehouse_live_value_payloads();

        // ── Phase 4: rebuild all derived structures ──────────────────
        self.rebuild_anchor_positions();

        self.orphaned_node_ids
            .trim_retained_capacity(OrphanedNodeIds::INITIAL_CAPACITY);
    }

    fn should_compact_gaps(old_len: usize, gap_count: usize, new_len: usize) -> bool {
        if gap_count == 0 {
            return false;
        }
        if old_len <= Self::EAGER_COMPACT_SLOT_LEN {
            return true;
        }
        gap_count >= new_len
            || (gap_count >= Self::FRACTIONAL_COMPACT_GAP_THRESHOLD
                && gap_count.saturating_mul(Self::FRACTIONAL_COMPACT_RATIO_DIVISOR) >= old_len)
    }

    fn rebuild_anchor_positions(&mut self) {
        let live_anchor_count = self
            .slots
            .iter()
            .filter(|slot| slot.anchor_id().is_valid())
            .count();
        let mut anchors = HashMap::default();
        anchors.reserve(live_anchor_count);
        for (position, slot) in self.slots.iter().enumerate() {
            let anchor = slot.anchor_id();
            if anchor.is_valid() {
                anchors.insert(anchor.0, position);
            }
        }
        self.anchors = anchors;
        self.free_anchor_ids = Vec::new();
        self.next_anchor_id.set(
            self.next_anchor_id.get().max(
                self.anchors
                    .keys()
                    .copied()
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1),
            ),
        );
    }
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SlotStorage implementation for SlotTable
// ═══════════════════════════════════════════════════════════════════════════

/// Baseline SlotStorage implementation using a gap-buffer strategy.
///
/// This is the reference / most-feature-complete backend, supporting:
/// - Gap-based slot reuse (preserving sibling state during conditional rendering)
/// - Anchor-based positional stability during group moves and insertions
/// - Efficient group skipping and recomposition via scope-based entry
/// - Batch anchor rebuilding for large structural changes
///
/// **Implementation Strategy:**
/// Uses UFCS (Uniform Function Call Syntax) to delegate to SlotTable's
/// inherent methods, avoiding infinite recursion while keeping the trait
/// implementation clean.
impl SlotStorage for SlotTable {
    type Group = GroupId;
    type ValueSlot = ValueSlotId;

    fn begin_group(&mut self, key: Key) -> StartGroup<Self::Group> {
        let idx = SlotTable::start(self, key);
        let restored = SlotTable::take_last_start_was_gap(self);
        StartGroup {
            group: GroupId(idx),
            anchor: self.slots[idx].anchor_id(),
            restored_from_gap: restored,
        }
    }

    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId) {
        SlotTable::set_group_scope(self, group.0, scope);
    }

    fn end_group(&mut self) {
        SlotTable::end(self);
    }

    fn skip_current_group(&mut self) {
        SlotTable::skip_current(self);
    }

    fn nodes_in_current_group(&self) -> Vec<NodeId> {
        SlotTable::node_ids_in_current_group(self)
    }

    fn begin_recranpose_at_anchor(
        &mut self,
        anchor: AnchorId,
        scope: ScopeId,
    ) -> Option<Self::Group> {
        SlotTable::start_recranpose_at_anchor(self, anchor, scope).map(GroupId)
    }

    fn end_recompose(&mut self) {
        SlotTable::end_recompose(self);
    }

    fn alloc_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot {
        let idx = SlotTable::use_value_slot(self, init);
        ValueSlotId(idx)
    }

    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T {
        SlotTable::read_value(self, slot.0)
    }

    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T {
        SlotTable::read_value_mut(self, slot.0)
    }

    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T) {
        SlotTable::write_value(self, slot.0, value);
    }

    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        SlotTable::remember(self, init)
    }

    fn peek_node(&self) -> Option<(NodeId, u32)> {
        SlotTable::peek_node(self)
    }

    fn record_node(&mut self, id: NodeId, gen: u32) {
        SlotTable::record_node(self, id, gen);
    }

    fn advance_after_node_read(&mut self) {
        SlotTable::advance_after_node_read(self);
    }

    fn step_back(&mut self) {
        SlotTable::step_back(self);
    }

    fn finalize_current_group(&mut self) -> bool {
        SlotTable::trim_to_cursor(self)
    }

    fn reset(&mut self) {
        SlotTable::reset(self);
    }

    fn flush(&mut self) {
        SlotTable::flush_anchors_if_dirty(self);
    }
}

impl Drop for SlotTable {
    fn drop(&mut self) {
        self.flush_pending_slot_drops();
        drop_slots_in_reverse(&mut self.slots);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChildReusePolicy, GapMetadata, GroupFrame, NodeSlotState, OrphanedNode, OrphanedNodeIds,
        Slot, SlotTable,
    };
    use crate::{AnchorId, Owned, RecomposeScope};

    #[test]
    fn large_slot_tables_grow_incrementally_instead_of_doubling() {
        let mut table = SlotTable::new();
        table.append_gap_slots(SlotTable::LARGE_GROWTH_THRESHOLD);

        table.grow_slots();

        assert_eq!(table.slots.len(), 40_960);
        assert!(table.slots.capacity() >= table.slots.len());

        table.grow_slots();

        assert_eq!(table.slots.len(), 51_200);
        assert!(table.slots.capacity() >= table.slots.len());
    }

    #[test]
    fn flushing_large_pending_slot_drops_releases_excess_capacity() {
        let mut table = SlotTable::new();
        for _ in 0..4096 {
            table.pending_slot_drops.push(Slot::default());
        }

        table.flush_pending_slot_drops();

        let stats = table.debug_stats();
        assert_eq!(stats.pending_slot_drops_len, 0);
        assert!(
            stats.pending_slot_drops_cap <= SlotTable::MIN_RETAINED_PENDING_SLOT_DROPS_CAPACITY * 4,
            "pending slot drop capacity stayed too large after flush: {stats:?}",
        );
    }

    #[test]
    fn draining_large_orphaned_node_ids_releases_excess_capacity() {
        let mut table = SlotTable::new();
        for node_id in 0..4096 {
            table
                .orphaned_node_ids
                .push(OrphanedNode::new(node_id, 0, AnchorId::INVALID));
        }

        let mut drained = Vec::new();
        table.drain_orphaned_node_ids_with(|node| drained.push(node));

        let stats = table.debug_stats();
        assert_eq!(stats.orphaned_node_ids_len, 0);
        assert_eq!(drained.len(), 4096);
        assert_eq!(drained[0], OrphanedNode::new(0, 0, AnchorId::INVALID));
        assert_eq!(drained[4095], OrphanedNode::new(4095, 0, AnchorId::INVALID));
        assert!(
            stats.orphaned_node_ids_cap <= OrphanedNodeIds::INITIAL_CAPACITY * 4,
            "orphaned node id capacity stayed too large after drain: {stats:?}",
        );
    }

    #[test]
    fn compaction_rehouses_surviving_value_payloads() {
        let mut table = SlotTable::new();
        let survivor_idx = table.use_value_slot(|| String::from("survivor"));
        let survivor_before = table.read_value::<String>(survivor_idx) as *const String;

        for index in 0..4096 {
            table.use_value_slot(|| format!("drop-{index}"));
        }

        table.cursor = 1;
        assert!(table.trim_to_cursor(), "test setup should produce gaps");
        table.compact();

        let survivor_after = table.read_value::<String>(0);
        let survivor_after_ptr = survivor_after as *const String;
        assert_eq!(survivor_after, "survivor");
        assert_ne!(
            survivor_before, survivor_after_ptr,
            "compaction should move surviving value payloads into fresh boxes"
        );
    }

    #[test]
    fn compaction_releases_peak_anchor_storage() {
        let mut table = SlotTable::new();
        table.use_value_slot(|| String::from("survivor"));
        for index in 0..4096 {
            table.use_value_slot(|| format!("drop-{index}"));
        }

        table.cursor = 1;
        assert!(table.trim_to_cursor(), "test setup should produce gaps");
        table.compact();

        let stats = table.debug_stats();
        assert!(
            stats.anchors_cap <= stats.slots_len.saturating_mul(4).max(8),
            "anchor storage stayed too large after compaction: {stats:?}",
        );
        assert!(
            stats.free_anchor_ids_cap <= 8,
            "free anchor id storage stayed too large after compaction: {stats:?}",
        );
    }

    #[test]
    fn compaction_reclaims_large_fractional_gap_blocks() {
        let mut table = SlotTable::new();

        for index in 0..1536 {
            table.use_value_slot(|| format!("slot-{index}"));
        }

        table.cursor = 1024;
        assert!(
            table.trim_to_cursor(),
            "test setup should produce a large gap block"
        );
        table.compact();

        assert_eq!(
            table.slots.len(),
            1024,
            "fractional large gap blocks should compact once hidden payloads become significant",
        );
    }

    #[test]
    fn compaction_preserves_anchor_identity_for_shifted_slots() {
        let mut table = SlotTable::new();

        table.reset();
        let first_group = table.start(1);
        table.set_group_scope(first_group, 11);
        table.use_value_slot(|| String::from("drop-a"));
        table.use_value_slot(|| String::from("drop-b"));
        table.end();

        let second_group = table.start(2);
        table.set_group_scope(second_group, 22);
        let (survivor_index, survivor_anchor) =
            table.remember_with_anchor(|| String::from("survivor"));
        table.end();
        table.flush_anchors_if_dirty();

        assert_eq!(
            table
                .read_value_by_anchor::<Owned<String>>(survivor_anchor)
                .map(|value| value.with(|text| text.clone())),
            Some(String::from("survivor"))
        );
        assert_eq!(table.resolve_anchor(survivor_anchor), Some(survivor_index));

        table.reset();
        let started = table
            .start_recranpose_at_anchor(table.group_anchor_at(first_group), 11)
            .expect("recompose scope should be found");
        assert_eq!(started, first_group);
        table.end_recompose();
        assert!(
            table.needs_compact,
            "shrinking the first group should create gaps"
        );

        table.compact();

        let shifted_index = table
            .resolve_anchor(survivor_anchor)
            .expect("survivor anchor should still resolve after compaction");
        assert!(
            shifted_index < survivor_index,
            "survivor slot should move left when leading gaps are compacted"
        );
        assert_eq!(
            table
                .read_value_by_anchor::<Owned<String>>(survivor_anchor)
                .map(|value| value.with(|text| text.clone())),
            Some(String::from("survivor"))
        );

        table
            .read_value_mut_by_anchor::<Owned<String>>(survivor_anchor)
            .expect("mutable anchor lookup should remain valid after compaction")
            .replace(String::from("survivor-updated"));
        assert_eq!(
            table
                .read_value::<Owned<String>>(shifted_index)
                .with(|text| text.clone()),
            "survivor-updated"
        );
    }

    #[test]
    fn sibling_search_does_not_steal_nested_matching_group() {
        let mut table = SlotTable::new();
        let root_key = crate::hash_key(&"root");
        let target_key = crate::location_key("target", 10, 20);
        let container_key = crate::location_key("container", 11, 21);

        table.reset();
        let root = table.start(root_key);
        table.set_group_scope(root, 1);

        let container = table.start(container_key);
        table.set_group_scope(container, 10);

        let nested = table.start(target_key);
        table.set_group_scope(nested, 22);
        table.use_value_slot(|| String::from("nested"));
        table.end();

        table.end();
        table.end();
        table.flush_anchors_if_dirty();

        let root_end = match table.slots[root] {
            Slot::Group { len, .. } => root + super::unpack_slot_len(len),
            _ => panic!("root should remain a group"),
        };

        table.cursor = container;
        table.group_stack.push(GroupFrame {
            key: root_key,
            start: root,
            end: root_end,
            child_reuse: ChildReusePolicy::Normal,
            fresh_body: false,
            gap_boundary_key: root_key,
        });

        let inserted = table.start(target_key);

        assert_eq!(inserted, container);
        assert_eq!(
            table.get_group_scope(inserted),
            None,
            "search should not steal a nested descendant group",
        );
    }

    #[test]
    fn explicit_keys_relocate_matching_sibling_groups() {
        let mut table = SlotTable::new();
        let root_key = crate::hash_key(&"root");
        let target_key = crate::hash_key(&"target");
        let other_key = crate::hash_key(&"other");

        table.reset();
        let root = table.start(root_key);
        table.set_group_scope(root, 1);

        let first = table.start(other_key);
        table.set_group_scope(first, 10);
        table.use_value_slot(|| String::from("other"));
        table.end();

        let later = table.start(target_key);
        table.set_group_scope(later, 22);
        table.use_value_slot(|| String::from("later"));
        table.end();

        table.end();
        table.flush_anchors_if_dirty();

        let root_end = match table.slots[root] {
            Slot::Group { len, .. } => root + super::unpack_slot_len(len),
            _ => panic!("root should remain a group"),
        };

        table.cursor = first;
        table.group_stack.push(GroupFrame {
            key: root_key,
            start: root,
            end: root_end,
            child_reuse: ChildReusePolicy::Normal,
            fresh_body: false,
            gap_boundary_key: root_key,
        });

        let relocated = table.start(target_key);

        assert_eq!(relocated, first);
        assert_eq!(
            table.get_group_scope(relocated),
            Some(22),
            "explicit keyed groups should preserve scope identity when reordered",
        );
    }

    #[test]
    fn fresh_parent_does_not_restore_matching_child_from_replaced_subtree() {
        let mut table = SlotTable::new();
        let root_key = crate::hash_key(&"root");
        let old_parent_key = crate::hash_key(&"old-parent");
        let new_parent_key = crate::hash_key(&"new-parent");
        let shared_child_key = crate::hash_key(&"shared-child");

        table.reset();
        let root = table.start(root_key);
        table.set_group_scope(root, 1);

        let old_parent = table.start(old_parent_key);
        table.set_group_scope(old_parent, 10);

        let old_child = table.start(shared_child_key);
        table.set_group_scope(old_child, 20);
        table.use_value_slot(|| String::from("old-child"));
        table.end();

        table.end();
        table.end();
        table.flush_anchors_if_dirty();

        table.reset();
        let reused_root = table.start(root_key);
        assert_eq!(reused_root, root);

        let fresh_parent = table.start(new_parent_key);
        assert_eq!(fresh_parent, old_parent);

        let fresh_child = table.start(shared_child_key);
        let fresh_value = table.use_value_slot(|| String::from("fresh-child"));

        assert_eq!(fresh_child, old_child);
        assert_eq!(
            table.get_group_scope(fresh_child),
            None,
            "a fresh parent must not restore a descendant from the subtree it replaced",
        );
        assert_eq!(
            fresh_value,
            old_child + 1,
            "fresh child should reuse the slot position but not the previous descendant value slot",
        );
        assert_eq!(
            table.read_value::<String>(fresh_value),
            "fresh-child",
            "fresh child must not inherit the replaced subtree's remembered values",
        );
    }

    #[test]
    fn end_recompose_marks_shrunk_group_tail_as_gaps_for_compaction() {
        let mut table = SlotTable::new();

        table.reset();
        let group = table.start(100);
        table.set_group_scope(group, 7);
        let scope_slot = table.use_value_slot(|| String::from("scope"));
        let kept_slot = table.use_value_slot(|| String::from("keep"));
        let dropped_slot = table.use_value_slot(|| String::from("drop"));
        table.end();
        table.flush_anchors_if_dirty();

        assert_eq!(table.read_value::<String>(scope_slot), "scope");
        assert_eq!(table.read_value::<String>(kept_slot), "keep");
        assert_eq!(table.read_value::<String>(dropped_slot), "drop");

        table.reset();
        let started = table
            .start_recranpose_at_anchor(table.group_anchor_at(group), 7)
            .expect("recompose scope should be found");
        assert_eq!(started, group);
        let reused_kept_slot = table.use_value_slot(|| String::from("keep"));
        assert_eq!(reused_kept_slot, kept_slot);
        table.end_recompose();

        assert!(
            table.needs_compact,
            "shrinking recompose should mark the old tail as gaps"
        );

        table.compact();

        assert_eq!(table.slots.len(), 3);
        assert_eq!(table.read_value::<String>(scope_slot), "scope");
        assert_eq!(table.read_value::<String>(kept_slot), "keep");
    }

    #[test]
    fn marking_existing_nested_gap_preserves_child_group_metadata() {
        let mut table = SlotTable::new();

        table.reset();
        let root = table.start(1);
        table.set_group_scope(root, 1);

        let parent = table.start(2);
        table.set_group_scope(parent, 2);

        let child = table.start(3);
        table.set_group_scope(child, 3);
        table.use_value_slot(|| String::from("child"));
        table.end();

        table.end();
        table.end();
        table.flush_anchors_if_dirty();

        let parent_end = match table.slots[parent] {
            Slot::Group { len, .. } => parent + super::unpack_slot_len(len),
            _ => panic!("parent should remain a group"),
        };

        assert!(
            table.mark_range_as_gaps(parent, parent_end, Some(root)),
            "initial mark should convert the subtree to gaps",
        );
        assert!(
            table.mark_range_as_gaps(parent, parent_end, Some(root)),
            "re-marking an existing gap subtree should still preserve child metadata",
        );

        assert!(
            matches!(table.slots[parent], Slot::Gap { .. }),
            "parent should be a preserved gap",
        );
        let parent_metadata = table
            .gap_metadata_at(parent)
            .expect("parent gap metadata should be preserved");
        assert_eq!(parent_metadata.group_key, Some(2));
        assert_eq!(parent_metadata.group_scope.to_option(), Some(2));
        assert_eq!(super::unpack_slot_len(parent_metadata.group_len), 3);

        assert!(
            matches!(table.slots[child], Slot::Gap { .. }),
            "child should be a preserved gap",
        );
        let child_metadata = table
            .gap_metadata_at(child)
            .expect("child gap metadata should be preserved");
        assert_eq!(
            child_metadata.group_key,
            Some(3),
            "child gap metadata must survive repeated ancestor gap marking",
        );
        assert_eq!(child_metadata.group_scope.to_option(), Some(3));
        assert_eq!(super::unpack_slot_len(child_metadata.group_len), 2);
    }

    #[test]
    fn start_recranpose_at_anchor_rejects_mismatched_scope_owner() {
        let mut table = SlotTable::new();
        let root_key = crate::hash_key(&"root");
        let group_key = crate::hash_key(&"group");

        table.reset();
        let root = table.start(root_key);
        table.set_group_scope(root, 1);

        let group = table.start(group_key);
        table.set_group_scope(group, 10);
        table.use_value_slot(|| String::from("first"));
        table.end();
        table.end();
        table.flush_anchors_if_dirty();

        let anchor = table.group_anchor_at(group);

        table.reset();
        let reused_root = table.start(root_key);
        assert_eq!(reused_root, root);
        let reused_group = table.start(group_key);
        assert_eq!(reused_group, group);
        table.set_group_scope(reused_group, 20);
        table.use_value_slot(|| String::from("second"));
        table.end();
        table.end();
        table.flush_anchors_if_dirty();

        table.reset();
        assert_eq!(
            table.start_recranpose_at_anchor(anchor, 10),
            None,
            "a stale scope must not enter a live group that now belongs to a different owner",
        );
        assert_eq!(
            table.start_recranpose_at_anchor(anchor, 20),
            Some(group),
            "the current owner should still enter the group through its anchor",
        );
    }

    #[test]
    fn gapped_group_preserves_child_value_slots_for_restore() {
        let mut table = SlotTable::new();

        table.reset();
        let root = table.start(1);
        let group = table.start(2);
        let value_slot = table.use_value_slot(|| String::from("persist"));
        table.end();
        table.end();
        table.flush_anchors_if_dirty();

        let group_end = match table.slots[group] {
            Slot::Group { len, .. } => group + super::unpack_slot_len(len),
            _ => panic!("group should remain a group"),
        };

        assert!(
            table.mark_range_as_gaps(group, group_end, Some(root)),
            "group should become hidden behind a gap",
        );
        assert!(
            matches!(table.slots[value_slot], Slot::Value { .. }),
            "group child value slots must survive while the parent group is hidden",
        );

        table.reset();
        let reused_root = table.start(1);
        assert_eq!(reused_root, root);
        let restored_group = table.start(2);
        assert_eq!(restored_group, group);
        let restored_slot = table.use_value_slot(|| String::from("new"));
        assert_eq!(restored_slot, value_slot);
        assert_eq!(table.read_value::<String>(restored_slot), "persist");
    }

    #[test]
    fn compaction_drops_preserved_child_values_inside_hidden_gap_group() {
        let mut table = SlotTable::new();

        table.reset();
        let root = table.start(1);
        let group = table.start(2);
        let _value_slot = table.use_value_slot(|| String::from("persist"));
        table.end();
        table.end();

        let group_end = match table.slots[group] {
            Slot::Group { len, .. } => group + super::unpack_slot_len(len),
            _ => panic!("group should remain a group"),
        };

        assert!(
            table.mark_range_as_gaps(group, group_end, Some(root)),
            "group should become hidden behind a gap",
        );
        table.compact();

        assert_eq!(
            table.slots.len(),
            1,
            "compaction should remove the entire hidden group block, including preserved child values",
        );
        match table.slots[0] {
            Slot::Group { len, .. } => assert_eq!(super::unpack_slot_len(len), 1),
            _ => panic!("root group should remain after compaction"),
        }
    }

    #[test]
    fn orphaned_node_state_reports_preserved_gap_nodes() {
        let mut table = SlotTable::new();
        let anchor = AnchorId(1);
        table.slots.push(Slot::Gap { anchor });
        table.register_anchor(anchor, 0);
        table.set_gap_metadata(
            anchor,
            GapMetadata {
                preserved_node: Some((42, 7)),
                ..GapMetadata::default()
            },
        );

        assert_eq!(
            table.orphaned_node_state(OrphanedNode::new(42, 7, anchor)),
            NodeSlotState::PreservedGap
        );
    }

    #[test]
    fn orphaned_node_state_reports_restored_active_node() {
        let mut table = SlotTable::new();
        let anchor = AnchorId(1);
        table.slots.push(Slot::Gap { anchor });
        table.register_anchor(anchor, 0);
        table.set_gap_metadata(
            anchor,
            GapMetadata {
                preserved_node: Some((42, 7)),
                ..GapMetadata::default()
            },
        );
        table.set_slot_tracked(
            0,
            Slot::Node {
                anchor,
                id: 42,
                gen: 7,
            },
        );

        assert_eq!(
            table.orphaned_node_state(OrphanedNode::new(42, 7, anchor)),
            NodeSlotState::Active
        );
    }

    #[test]
    fn orphaned_node_state_reports_missing_after_anchor_is_gone() {
        let mut table = SlotTable::new();
        let anchor = AnchorId(1);
        table.slots.push(Slot::Node {
            anchor,
            id: 42,
            gen: 7,
        });
        table.register_anchor(anchor, 0);
        table.free_anchor(anchor);

        assert_eq!(
            table.orphaned_node_state(OrphanedNode::new(42, 7, anchor)),
            NodeSlotState::Missing
        );
    }

    #[test]
    fn exact_live_group_reuses_even_when_parent_restore_restricts_gap_search() {
        let mut table = SlotTable::new();

        table.reset();
        let root = table.start(1);
        let child = table.start(2);
        table.set_group_scope(child, 2);
        let grandchild = table.start(3);
        table.set_group_scope(grandchild, 3);
        table.end();
        table.end();
        table.end();
        table.flush_anchors_if_dirty();

        table.reset();
        table.cursor = child;
        table.group_stack.push(GroupFrame {
            key: 1,
            start: root,
            end: grandchild + 1,
            child_reuse: ChildReusePolicy::ParentRestoredFromGap,
            fresh_body: true,
            gap_boundary_key: 1,
        });

        let reused_child = table.start(2);
        assert_eq!(
            reused_child, child,
            "restored parents must still reuse an exact live child group at the cursor",
        );
        assert_eq!(
            table.get_group_scope(reused_child),
            Some(2),
            "exact live group reuse must preserve the child scope",
        );

        let reused_grandchild = table.start(3);
        assert_eq!(
            reused_grandchild, grandchild,
            "once the exact live child group is reused, its live descendants should stay aligned",
        );
        assert_eq!(table.get_group_scope(reused_grandchild), Some(3));
    }

    #[test]
    fn exact_live_node_slot_reuses_even_when_parent_restore_restricts_other_live_slots() {
        let mut table = SlotTable::new();

        table.reset();
        let root = table.start(1);
        let node_slot = table.cursor;
        table.record_node(42, 7);
        table.end();
        table.flush_anchors_if_dirty();

        table.reset();
        table.cursor = node_slot;
        table.group_stack.push(GroupFrame {
            key: 1,
            start: root,
            end: node_slot + 1,
            child_reuse: ChildReusePolicy::ParentRestoredFromGap,
            fresh_body: true,
            gap_boundary_key: 1,
        });

        assert_eq!(
            table.peek_node(),
            Some((42, 7)),
            "restored parents must still expose an exact live node slot at the cursor",
        );

        table.record_node(42, 7);

        assert_eq!(
            table.cursor,
            node_slot + 1,
            "recording the same node should reuse the existing slot instead of forcing a gap",
        );
        match table.slots.get(node_slot) {
            Some(Slot::Node { id, gen, .. }) => assert_eq!((*id, *gen), (42, 7)),
            _ => panic!("node slot should stay live after exact reuse"),
        }
    }

    #[test]
    fn exact_live_value_slot_reuses_when_parent_is_restored_from_gap() {
        let mut table = SlotTable::new();

        table.reset();
        let root = table.start(1);
        let value_slot = table.use_value_slot(|| String::from("persist"));
        table.end();
        table.flush_anchors_if_dirty();

        table.reset();
        table.cursor = value_slot;
        table.group_stack.push(GroupFrame {
            key: 1,
            start: root,
            end: value_slot + 1,
            child_reuse: ChildReusePolicy::ParentRestoredFromGap,
            fresh_body: true,
            gap_boundary_key: 1,
        });

        let reused_slot = table.use_value_slot(|| String::from("fresh"));
        assert_eq!(
            reused_slot, value_slot,
            "restored parents must reuse an exact live value slot at the cursor",
        );
        assert_eq!(
            table.read_value::<String>(reused_slot),
            "persist",
            "restored parents must keep their remembered value instead of recreating it",
        );
    }

    #[test]
    fn marking_scope_slot_as_gap_deactivates_scope() {
        let runtime = crate::runtime::TestRuntime::new();
        let scope = RecomposeScope::new(runtime.handle());
        let mut table = SlotTable::new();

        table.reset();
        let group = table.start(1);
        let _scope_slot = table.use_value_slot(|| scope.clone());
        table.end();

        assert!(scope.is_active(), "scope should start active");

        table.mark_range_as_gaps(group, group + 2, None);

        assert!(
            !scope.is_active(),
            "scope stored in a slot must deactivate once that slot is converted into a gap"
        );
    }
}

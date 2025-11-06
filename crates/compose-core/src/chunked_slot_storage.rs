//! Chunked slot storage backend that avoids large rotate operations.
//!
//! This backend divides the slot array into fixed-size chunks (256 slots each),
//! allowing insertions and deletions to only shift slots within or between
//! adjacent chunks rather than rotating the entire storage. This improves
//! performance for large compositions with frequent insertions.
//!
//! ## Implementation Details
//!
//! - **Chunk size**: Fixed at 256 slots per chunk for optimal balance between
//!   overhead and shift performance.
//! - **Insertion strategy**: When inserting at a non-gap position, finds the
//!   nearest gap and shifts slots to make room. Falls back to overwrite if no
//!   gap is found nearby.
//! - **Gap restoration**: When beginning a group at a gap slot with matching key,
//!   restores the group with its preserved length and scope, setting
//!   `force_children_recompose = true`.
//! - **Anchor lifecycle**:
//!   1. Created with `alloc_anchor()` when a slot is allocated
//!   2. Marked dirty via `anchors_dirty = true` when slots are shifted
//!   3. Rebuilt via `rebuild_anchors()` during `flush()`
//!
//! ## Trade-offs
//!
//! - **Pros**: Better insertion performance for large compositions
//! - **Cons**: Higher memory overhead (fixed chunk sizes), more complex indexing

use crate::{
    slot_storage::{GroupId, SlotStorage, StartGroup, ValueSlotId},
    AnchorId, Key, NodeId, Owned, ScopeId,
};
use std::any::Any;
use std::cell::Cell;

/// Size of each chunk in slots. Tuned for balance between chunk overhead
/// and shift performance.
const CHUNK_SIZE: usize = 256;

/// Chunked slot storage implementation.
///
/// Uses a Vec of fixed-size chunks to store slots, avoiding the O(n) rotate
/// operations needed when inserting near the start of a large flat Vec.
#[derive(Default)]
pub struct ChunkedSlotStorage {
    /// Storage chunks, each up to CHUNK_SIZE slots.
    chunks: Vec<Vec<ChunkedSlot>>,
    /// Global cursor position (linear index across all chunks).
    cursor: usize,
    /// Group stack tracking current composition nesting.
    group_stack: Vec<GroupFrame>,
    /// Anchor ID → global slot position mapping.
    anchors: Vec<usize>,
    /// Whether anchors need rebuilding.
    anchors_dirty: bool,
    /// Counter for allocating unique anchor IDs.
    next_anchor_id: Cell<usize>,
    /// Tracks whether the most recent begin_group reused a gap.
    last_start_was_gap: bool,
}

struct GroupFrame {
    key: Key,
    start: usize,
    end: usize,
    force_children_recompose: bool,
}

enum ChunkedSlot {
    Group {
        key: Key,
        anchor: AnchorId,
        len: usize,
        scope: Option<ScopeId>,
        has_gap_children: bool,
    },
    Value {
        anchor: AnchorId,
        data: Box<dyn Any>,
    },
    Node {
        anchor: AnchorId,
        id: NodeId,
    },
    Gap {
        anchor: AnchorId,
        group_key: Option<Key>,
        group_scope: Option<ScopeId>,
        group_len: usize,
    },
}

impl ChunkedSlot {
    fn anchor_id(&self) -> AnchorId {
        match self {
            ChunkedSlot::Group { anchor, .. } => *anchor,
            ChunkedSlot::Value { anchor, .. } => *anchor,
            ChunkedSlot::Node { anchor, .. } => *anchor,
            ChunkedSlot::Gap { anchor, .. } => *anchor,
        }
    }

    fn as_value<T: 'static>(&self) -> &T {
        match self {
            ChunkedSlot::Value { data, .. } => data
                .downcast_ref::<T>()
                .expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }

    fn as_value_mut<T: 'static>(&mut self) -> &mut T {
        match self {
            ChunkedSlot::Value { data, .. } => data
                .downcast_mut::<T>()
                .expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }
}

impl Default for ChunkedSlot {
    fn default() -> Self {
        ChunkedSlot::Gap {
            anchor: AnchorId::INVALID,
            group_key: None,
            group_scope: None,
            group_len: 0,
        }
    }
}

impl ChunkedSlotStorage {
    pub fn new() -> Self {
        Self {
            next_anchor_id: Cell::new(1), // Start at 1 (0 is INVALID)
            ..Default::default()
        }
    }

    /// Get total number of slots across all chunks.
    fn total_slots(&self) -> usize {
        self.chunks.iter().map(|c| c.len()).sum()
    }

    /// Convert global index to (chunk_index, offset).
    fn global_to_chunk(&self, global: usize) -> (usize, usize) {
        let mut remaining = global;
        for (chunk_idx, chunk) in self.chunks.iter().enumerate() {
            if remaining < chunk.len() {
                return (chunk_idx, remaining);
            }
            remaining -= chunk.len();
        }
        // Past the end
        (self.chunks.len(), 0)
    }

    /// Get a reference to the slot at global index.
    fn get_slot(&self, global: usize) -> Option<&ChunkedSlot> {
        let (chunk_idx, offset) = self.global_to_chunk(global);
        self.chunks.get(chunk_idx)?.get(offset)
    }

    /// Get a mutable reference to the slot at global index.
    fn get_slot_mut(&mut self, global: usize) -> Option<&mut ChunkedSlot> {
        let (chunk_idx, offset) = self.global_to_chunk(global);
        self.chunks.get_mut(chunk_idx)?.get_mut(offset)
    }

    /// Allocate a new anchor ID.
    fn alloc_anchor(&self) -> AnchorId {
        let id = self.next_anchor_id.get();
        self.next_anchor_id.set(id + 1);
        AnchorId::new(id)
    }

    /// Ensure capacity at cursor by adding gap slots if needed.
    fn ensure_capacity(&mut self) {
        if self.chunks.is_empty() {
            // Initialize first chunk
            let mut chunk = Vec::with_capacity(CHUNK_SIZE);
            chunk.resize_with(CHUNK_SIZE, ChunkedSlot::default);
            self.chunks.push(chunk);
        }

        let total = self.total_slots();
        if self.cursor >= total {
            // Need more chunks
            let mut chunk = Vec::with_capacity(CHUNK_SIZE);
            chunk.resize_with(CHUNK_SIZE, ChunkedSlot::default);
            self.chunks.push(chunk);
        }
    }

    /// Update all group frame bounds to include the current cursor position.
    /// This ensures frames grow as we allocate more content than initially expected.
    fn update_group_bounds(&mut self) {
        for frame in &mut self.group_stack {
            if frame.end < self.cursor {
                frame.end = self.cursor;
            }
        }
    }

    /// Insert a slot at the cursor position, shifting within/between chunks.
    /// This implements proper insertion semantics: if the target is a gap, reuse it;
    /// otherwise, shift slots to make room.
    fn insert_at_cursor(&mut self, slot: ChunkedSlot) {
        self.ensure_capacity();

        // Fast path: if current slot is a gap (any gap, regardless of anchor), reuse it
        if let Some(existing) = self.get_slot(self.cursor) {
            if matches!(existing, ChunkedSlot::Gap { .. }) {
                *self.get_slot_mut(self.cursor).unwrap() = slot;
                // Update group end to account for this slot
                if let Some(frame) = self.group_stack.last_mut() {
                    if self.cursor >= frame.end {
                        frame.end = self.cursor + 1;
                    }
                }
                self.anchors_dirty = true;
                return;
            }
        }

        // Need to insert: find a gap to the right and shift slots
        if let Some(gap_pos) = self.find_gap_near_cursor() {
            // Shift slots from cursor to gap_pos-1 to the right by 1
            // Use shift_across_chunks for all cases (handles both same-chunk and cross-chunk)
            self.shift_across_chunks(self.cursor, gap_pos);

            // Now insert at cursor
            *self.get_slot_mut(self.cursor).unwrap() = slot;

            // Update all group frames that overlap with the shifted region.
            // This assumes frames are in creation order and fully cover their ranges.
            // Frames starting at or after cursor: shift both start and end
            // Frames containing cursor: extend end only
            for frame in &mut self.group_stack {
                if frame.start >= self.cursor {
                    frame.start += 1;
                    frame.end += 1;
                } else if frame.end > self.cursor {
                    frame.end += 1;
                }
            }

            self.anchors_dirty = true;
        } else {
            // No gap nearby: just overwrite (fallback behavior)
            if self.cursor < self.total_slots() {
                *self.get_slot_mut(self.cursor).unwrap() = slot;
                if let Some(frame) = self.group_stack.last_mut() {
                    if self.cursor >= frame.end {
                        frame.end = self.cursor + 1;
                    }
                }
                self.anchors_dirty = true;
            }
        }
    }

    /// Shift slots across chunks from start to end (exclusive).
    /// Ensures capacity exists for all destination positions before shifting.
    fn shift_across_chunks(&mut self, start: usize, end: usize) {
        // Ensure we have capacity for the rightmost destination (end position)
        while self.total_slots() <= end {
            let mut chunk = Vec::with_capacity(CHUNK_SIZE);
            chunk.resize_with(CHUNK_SIZE, ChunkedSlot::default);
            self.chunks.push(chunk);
        }

        // Move slots one by one from end-1 down to start using mem::replace
        for i in (start..end).rev() {
            let (src_chunk, src_offset) = self.global_to_chunk(i);
            let (dst_chunk, dst_offset) = self.global_to_chunk(i + 1);

            // Use mem::replace to move without cloning
            let temp = std::mem::replace(
                &mut self.chunks[src_chunk][src_offset],
                ChunkedSlot::default()
            );
            // Capacity is guaranteed by the loop above
            self.chunks[dst_chunk][dst_offset] = temp;
        }
    }

    /// Rebuild anchor positions by scanning all slots.
    fn rebuild_anchors(&mut self) {
        if !self.anchors_dirty {
            return;
        }

        // Clear existing anchor map
        for pos in self.anchors.iter_mut() {
            *pos = usize::MAX;
        }

        // Scan all slots and update anchor positions
        let mut global_idx = 0;
        for chunk in &self.chunks {
            for slot in chunk {
                let anchor = slot.anchor_id();
                if anchor.is_valid() {
                    let id = anchor.0;
                    if id >= self.anchors.len() {
                        self.anchors.resize(id + 1, usize::MAX);
                    }
                    self.anchors[id] = global_idx;
                }
                global_idx += 1;
            }
        }

        self.anchors_dirty = false;
    }

    /// Lookup the current position of an anchor ID.
    /// Returns None if the anchor is not found or invalid.
    fn lookup_anchor_position(&self, anchor_id: usize) -> Option<usize> {
        if anchor_id < self.anchors.len() {
            let pos = self.anchors[anchor_id];
            if pos != usize::MAX {
                return Some(pos);
            }
        }
        None
    }

    /// Find a gap slot near the cursor.
    /// Accepts any gap slot (structural or finalized) for reuse.
    fn find_gap_near_cursor(&self) -> Option<usize> {
        // Look forward from cursor
        const SCAN_LIMIT: usize = 128;
        for offset in 0..SCAN_LIMIT {
            let pos = self.cursor + offset;
            if let Some(slot) = self.get_slot(pos) {
                // Accept any gap for reuse (regardless of anchor value)
                if matches!(slot, ChunkedSlot::Gap { .. }) {
                    return Some(pos);
                }
            }
        }
        None
    }

    /// Start a new group at the cursor.
    fn start_group(&mut self, key: Key) -> (usize, bool) {
        self.ensure_capacity();

        // Check if current slot is already a group with matching key - reuse it
        if let Some(slot) = self.get_slot(self.cursor) {
            if let ChunkedSlot::Group {
                key: existing_key,
                len,
                has_gap_children,
                ..
            } = slot
            {
                if *existing_key == key {
                    // Reuse existing group
                    let group_len = *len;
                    let had_gap_children = *has_gap_children;

                    // Clear gap children flag if present
                    if had_gap_children {
                        if let Some(ChunkedSlot::Group {
                            has_gap_children: ref mut flag,
                            ..
                        }) = self.get_slot_mut(self.cursor)
                        {
                            *flag = false;
                        }
                    }

                    let start = self.cursor;
                    self.cursor += 1;
                    self.group_stack.push(GroupFrame {
                        key,
                        start,
                        end: start + group_len + 1,
                        force_children_recompose: had_gap_children,
                    });
                    self.update_group_bounds();
                    self.last_start_was_gap = false;
                    return (start, false);
                }
            }
        }

        // Check if current slot is a gap group we can restore
        if let Some(slot) = self.get_slot(self.cursor) {
            if let ChunkedSlot::Gap {
                group_key: Some(gap_key),
                group_scope,
                group_len,
                anchor: gap_anchor,
            } = slot
            {
                if *gap_key == key {
                    // Restore the gap group, reusing the old anchor or creating new one
                    let anchor = if gap_anchor.is_valid() {
                        *gap_anchor
                    } else {
                        self.alloc_anchor()
                    };
                    let scope = *group_scope;
                    let len = *group_len;
                    *self.get_slot_mut(self.cursor).unwrap() = ChunkedSlot::Group {
                        key,
                        anchor,
                        len,
                        scope,
                        has_gap_children: true,
                    };

                    let start = self.cursor;
                    self.cursor += 1;
                    // Set frame.end to start + len + 1 to account for the group slot itself
                    // The group slot at `start` contains children from start+1 to start+len
                    self.group_stack.push(GroupFrame {
                        key,
                        start,
                        end: start + len + 1,
                        force_children_recompose: true,
                    });
                    self.update_group_bounds();
                    self.last_start_was_gap = true;
                    self.anchors_dirty = true;
                    return (start, true);
                }
            }
        }

        // Create new group
        let anchor = self.alloc_anchor();
        let slot = ChunkedSlot::Group {
            key,
            anchor,
            len: 0,
            scope: None,
            has_gap_children: false,
        };

        self.insert_at_cursor(slot);
        let start = self.cursor;
        self.cursor += 1;
        self.group_stack.push(GroupFrame {
            key,
            start,
            end: start,
            force_children_recompose: false,
        });
        self.update_group_bounds();
        self.last_start_was_gap = false;
        (start, false)
    }

    /// End the current group (internal implementation).
    fn do_end_group(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            let len = self.cursor.saturating_sub(frame.start + 1);
            if let Some(slot) = self.get_slot_mut(frame.start) {
                if let ChunkedSlot::Group { len: slot_len, .. } = slot {
                    *slot_len = len;
                }
            }
        }
    }

    /// Skip over the current group (internal implementation).
    fn do_skip_current_group(&mut self) {
        if let Some(frame) = self.group_stack.last() {
            let total = self.total_slots();
            self.cursor = frame.end.min(total);
        }
    }

    /// Finalize current group by marking unreached tail as gaps (internal implementation).
    fn do_finalize_current_group(&mut self) -> bool {
        let frame_end = match self.group_stack.last() {
            Some(frame) => frame.end,
            None => {
                // Root-level finalization: mark everything from cursor to end as gaps
                let total = self.total_slots();
                if self.cursor >= total {
                    return false;
                }
                let mut marked = false;
                while self.cursor < total {
                    if let Some(slot) = self.get_slot_mut(self.cursor) {
                        let anchor = slot.anchor_id();
                        let (group_key, group_scope, group_len) = match slot {
                            ChunkedSlot::Group { key, scope, len, .. } => (Some(*key), *scope, *len),
                            _ => (None, None, 0),
                        };
                        *slot = ChunkedSlot::Gap {
                            anchor,
                            group_key,
                            group_scope,
                            group_len,
                        };
                        marked = true;
                    }
                    self.cursor += 1;
                }
                // Mark anchors dirty so flush() rebuilds the anchor map
                self.anchors_dirty = true;
                return marked;
            }
        };

        let mut marked = false;
        while self.cursor < frame_end {
            if let Some(slot) = self.get_slot_mut(self.cursor) {
                // Convert to gap
                let anchor = slot.anchor_id();
                let (group_key, group_scope, group_len) = match slot {
                    ChunkedSlot::Group { key, scope, len, .. } => (Some(*key), *scope, *len),
                    _ => (None, None, 0),
                };
                *slot = ChunkedSlot::Gap {
                    anchor,
                    group_key,
                    group_scope,
                    group_len,
                };
                marked = true;
            }
            self.cursor += 1;
        }

        if let Some(frame) = self.group_stack.last_mut() {
            frame.end = self.cursor;
        }
        marked
    }
}

impl SlotStorage for ChunkedSlotStorage {
    type Group = GroupId;
    type ValueSlot = ValueSlotId;

    fn begin_group(&mut self, key: Key) -> StartGroup<Self::Group> {
        let (idx, restored) = self.start_group(key);
        StartGroup {
            group: GroupId::new(idx),
            restored_from_gap: restored,
        }
    }

    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId) {
        if let Some(slot) = self.get_slot_mut(group.index()) {
            if let ChunkedSlot::Group {
                scope: slot_scope, ..
            } = slot
            {
                *slot_scope = Some(scope);
            }
        }
    }

    fn end_group(&mut self) {
        self.do_end_group();
    }

    fn skip_current_group(&mut self) {
        self.do_skip_current_group();
    }

    fn nodes_in_current_group(&self) -> Vec<NodeId> {
        // Scan current group for nodes
        let mut nodes = Vec::new();
        if let Some(frame) = self.group_stack.last() {
            for pos in (frame.start + 1)..frame.end {
                if let Some(ChunkedSlot::Node { id, .. }) = self.get_slot(pos) {
                    nodes.push(*id);
                }
            }
        }
        nodes
    }

    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group> {
        // Linear scan to find group with this scope
        for global_idx in 0..self.total_slots() {
            if let Some(ChunkedSlot::Group {
                scope: Some(s),
                len,
                key,
                ..
            }) = self.get_slot(global_idx)
            {
                if *s == scope {
                    // Push group frame and position cursor inside the group
                    let frame = GroupFrame {
                        key: *key,
                        start: global_idx,
                        end: global_idx + len,
                        force_children_recompose: false,
                    };
                    self.group_stack.push(frame);
                    self.cursor = global_idx + 1;

                    // Skip over first value slot if present (matches baseline behavior)
                    if matches!(self.get_slot(self.cursor), Some(ChunkedSlot::Value { .. })) {
                        self.cursor += 1;
                    }

                    return Some(GroupId::new(global_idx));
                }
            }
        }
        None
    }

    fn end_recompose(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            self.cursor = frame.end;
        }
    }

    fn alloc_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot {
        self.ensure_capacity();

        // Check if current slot is a reusable value slot
        if let Some(ChunkedSlot::Value { data, .. }) = self.get_slot(self.cursor) {
            if data.is::<T>() {
                let slot_id = ValueSlotId::new(self.cursor);
                self.cursor += 1;
                self.update_group_bounds();
                return slot_id;
            }
        }

        // Check if the slot is a Gap that we can replace
        if matches!(self.get_slot(self.cursor), Some(ChunkedSlot::Gap { .. })) {
            let anchor = self.alloc_anchor();
            let slot = ChunkedSlot::Value {
                anchor,
                data: Box::new(init()),
            };
            *self.get_slot_mut(self.cursor).unwrap() = slot;
            let slot_id = ValueSlotId::new(self.cursor);
            self.cursor += 1;
            self.update_group_bounds();
            self.anchors_dirty = true;
            return slot_id;
        }

        // Create new value slot (type mismatch or other slot type)
        let anchor = self.alloc_anchor();
        let slot = ChunkedSlot::Value {
            anchor,
            data: Box::new(init()),
        };
        self.insert_at_cursor(slot);
        let slot_id = ValueSlotId::new(self.cursor);
        self.cursor += 1;
        self.update_group_bounds();
        slot_id
    }

    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T {
        self.get_slot(slot.index())
            .expect("value slot not found")
            .as_value()
    }

    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T {
        self.get_slot_mut(slot.index())
            .expect("value slot not found")
            .as_value_mut()
    }

    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T) {
        if let Some(slot_mut) = self.get_slot_mut(slot.index()) {
            if let ChunkedSlot::Value { data, .. } = slot_mut {
                *data = Box::new(value);
            }
        }
    }

    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let slot = self.alloc_value_slot(|| Owned::new(init()));
        self.read_value::<Owned<T>>(slot).clone()
    }

    fn peek_node(&self) -> Option<NodeId> {
        if let Some(ChunkedSlot::Node { id, .. }) = self.get_slot(self.cursor) {
            Some(*id)
        } else {
            None
        }
    }

    fn record_node(&mut self, id: NodeId) {
        self.ensure_capacity();
        let anchor = self.alloc_anchor();
        let slot = ChunkedSlot::Node { anchor, id };
        self.insert_at_cursor(slot);
        self.cursor += 1;
    }

    fn advance_after_node_read(&mut self) {
        self.cursor += 1;
    }

    fn step_back(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn finalize_current_group(&mut self) -> bool {
        self.do_finalize_current_group()
    }

    fn reset(&mut self) {
        self.cursor = 0;
        self.group_stack.clear();
    }

    fn flush(&mut self) {
        self.rebuild_anchors();
    }
}

impl ChunkedSlotStorage {
    /// Debug method to dump all groups.
    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        let mut result = Vec::new();
        for global_idx in 0..self.total_slots() {
            if let Some(ChunkedSlot::Group { key, len, scope, .. }) = self.get_slot(global_idx) {
                result.push((global_idx, *key, *scope, *len));
            }
        }
        result
    }

    /// Debug method to dump all slots.
    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        let mut result = Vec::new();
        for global_idx in 0..self.total_slots() {
            let desc = match self.get_slot(global_idx) {
                Some(ChunkedSlot::Group { key, scope, len, has_gap_children, .. }) => {
                    format!("Group(key={}, scope={:?}, len={}, gaps={})", key, scope, len, has_gap_children)
                }
                Some(ChunkedSlot::Value { .. }) => "Value".to_string(),
                Some(ChunkedSlot::Node { id, .. }) => format!("Node(id={})", id),
                Some(ChunkedSlot::Gap { group_key, group_scope, group_len, .. }) => {
                    if let Some(key) = group_key {
                        format!("Gap(was_group_key={}, scope={:?})", key, group_scope)
                    } else {
                        "Gap".to_string()
                    }
                }
                None => "Empty".to_string(),
            };
            result.push((global_idx, desc));
        }
        result
    }
}

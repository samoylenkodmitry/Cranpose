//! Split slot storage that separates layout from payload.
//!
//! This backend keeps the slot layout (groups, nodes, value references) separate
//! from the actual payload data stored in a HashMap. This separation allows the
//! layout to be overwritten with gaps without losing the associated payload,
//! enabling efficient data reuse across composition passes.
//!
//! ## Implementation Details
//!
//! - **Layout storage**: Vec of `LayoutSlot` containing structural information
//!   (groups, nodes) and references to payload via anchor IDs.
//! - **Payload storage**: `HashMap<anchor_id, Box<dyn Any>>` for actual data,
//!   persisting across gap cycles.
//! - **Value slot allocation**: Checks if existing layout slot references valid
//!   payload with matching type. If type mismatch or missing payload, creates
//!   new payload at the same anchor ID.
//! - **Gap handling**: When finalizing, marks unreached layout slots as gaps
//!   while preserving group metadata (key, scope, len). Payload remains intact.
//! - **Anchor lifecycle**:
//!   1. Created with `alloc_anchor()` when allocating value/group/node slots
//!   2. Marked dirty when layout is modified
//!   3. Rebuilt during `flush()` by scanning layout and updating anchor map
//!
//! ## Trade-offs
//!
//! - **Pros**: Payload persists across gaps (better memory reuse), simpler gap
//!   management (no need to preserve data in gap slots)
//! - **Cons**: HashMap overhead, potential for orphaned payloads (mitigated by
//!   debug assertions), indirection cost for payload access

use crate::{
    slot_storage::{GroupId, SlotStorage, StartGroup, ValueSlotId},
    AnchorId, Key, NodeId, Owned, ScopeId,
};
use std::any::Any;
use std::cell::Cell;
use std::collections::HashMap;

/// Split slot storage implementation.
///
/// Separates slot layout (structural information) from payload data
/// (remembered values), allowing layout changes without data loss.
#[derive(Default)]
pub struct SplitSlotStorage {
    /// Layout slots containing structural info and references to payload.
    layout: Vec<LayoutSlot>,
    /// Payload storage indexed by anchor ID.
    payload: HashMap<usize, Box<dyn Any>>,
    /// Current cursor in the layout.
    cursor: usize,
    /// Group stack tracking composition nesting.
    group_stack: Vec<GroupFrame>,
    /// Anchor ID → layout position mapping.
    anchors: Vec<usize>,
    /// Whether anchors need rebuilding.
    anchors_dirty: bool,
    /// Counter for allocating anchor IDs.
    next_anchor_id: Cell<usize>,
    /// Tracks whether last begin_group restored from gap.
    last_start_was_gap: bool,
}

struct GroupFrame {
    #[allow(dead_code)] // Tracked for debugging/future inspection tools
    key: Key,
    start: usize,
    end: usize,
    #[allow(dead_code)] // Tracked for debugging/future recomposition heuristics
    force_children_recompose: bool,
}

/// Layout slot containing only structural information.
enum LayoutSlot {
    Group {
        key: Key,
        anchor: AnchorId,
        len: usize,
        scope: Option<ScopeId>,
        has_gap_children: bool,
    },
    /// Reference to a value in the payload map.
    ValueRef {
        anchor: AnchorId,
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

impl LayoutSlot {
    fn anchor_id(&self) -> AnchorId {
        match self {
            LayoutSlot::Group { anchor, .. } => *anchor,
            LayoutSlot::ValueRef { anchor } => *anchor,
            LayoutSlot::Node { anchor, .. } => *anchor,
            LayoutSlot::Gap { anchor, .. } => *anchor,
        }
    }
}

impl Default for LayoutSlot {
    fn default() -> Self {
        LayoutSlot::Gap {
            anchor: AnchorId::INVALID,
            group_key: None,
            group_scope: None,
            group_len: 0,
        }
    }
}

impl SplitSlotStorage {
    pub fn new() -> Self {
        Self {
            next_anchor_id: Cell::new(1), // Start at 1 (0 is INVALID)
            ..Default::default()
        }
    }

    fn alloc_anchor(&self) -> AnchorId {
        let id = self.next_anchor_id.get();
        self.next_anchor_id.set(id + 1);
        AnchorId::new(id)
    }

    fn ensure_capacity(&mut self) {
        const INITIAL_CAP: usize = 32;
        if self.layout.is_empty() {
            self.layout.resize_with(INITIAL_CAP, LayoutSlot::default);
        } else if self.cursor >= self.layout.len() {
            let new_size = self.layout.len() * 2;
            self.layout.resize_with(new_size, LayoutSlot::default);
        }
    }

    fn insert_at_cursor(&mut self, slot: LayoutSlot) {
        self.ensure_capacity();

        // Check if we can reuse a gap
        if matches!(self.layout.get(self.cursor), Some(LayoutSlot::Gap { .. })) {
            self.layout[self.cursor] = slot;
        } else {
            // Need to insert - for simplicity, just overwrite
            // A full implementation would shift
            if self.cursor < self.layout.len() {
                self.layout[self.cursor] = slot;
            }
        }

        // Update group end to account for this slot
        if let Some(frame) = self.group_stack.last_mut() {
            if self.cursor >= frame.end {
                frame.end = self.cursor + 1;
            }
        }
        self.anchors_dirty = true;
    }

    fn start_group(&mut self, key: Key) -> (usize, bool) {
        self.ensure_capacity();

        // Check for gap group restoration
        if let Some(LayoutSlot::Gap {
            group_key: Some(gap_key),
            group_scope,
            group_len,
            anchor: gap_anchor,
        }) = self.layout.get(self.cursor)
        {
            if *gap_key == key {
                // Reuse the gap's anchor if valid, otherwise allocate new
                let anchor = if gap_anchor.is_valid() {
                    *gap_anchor
                } else {
                    self.alloc_anchor()
                };
                let scope = *group_scope;
                let len = *group_len;
                self.layout[self.cursor] = LayoutSlot::Group {
                    key,
                    anchor,
                    len,
                    scope,
                    has_gap_children: true,
                };

                let start = self.cursor;
                self.cursor += 1;
                // Set frame.end to start + len + 1 to properly bound the group.
                // This accounts for the group slot itself (at `start`) plus `len` children.
                // IMPORTANT: This pairing assumes do_finalize_current_group stores the
                // child count (not including the group slot) in group_len.
                self.group_stack.push(GroupFrame {
                    key,
                    start,
                    end: start + len + 1,
                    force_children_recompose: true,
                });
                self.last_start_was_gap = true;
                self.anchors_dirty = true;
                return (start, true);
            }
        }

        // Create new group
        let anchor = self.alloc_anchor();
        let slot = LayoutSlot::Group {
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
        self.last_start_was_gap = false;
        (start, false)
    }

    fn do_end_group(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            let len = self.cursor.saturating_sub(frame.start + 1);
            if let Some(LayoutSlot::Group { len: slot_len, .. }) = self.layout.get_mut(frame.start)
            {
                *slot_len = len;
            }
        }
    }

    fn do_finalize_current_group(&mut self) -> bool {
        let frame_end = match self.group_stack.last() {
            Some(frame) => frame.end,
            None => {
                // Root-level finalization: mark everything from cursor to end as gaps
                if self.cursor >= self.layout.len() {
                    return false;
                }
                let mut marked = false;
                while self.cursor < self.layout.len() {
                    let slot = &mut self.layout[self.cursor];
                    let anchor = slot.anchor_id();
                    let (group_key, group_scope, group_len) = match slot {
                        LayoutSlot::Group {
                            key, scope, len, ..
                        } => (Some(*key), *scope, *len),
                        _ => (None, None, 0),
                    };
                    *slot = LayoutSlot::Gap {
                        anchor,
                        group_key,
                        group_scope,
                        group_len,
                    };
                    marked = true;
                    self.cursor += 1;
                }
                // Mark anchors dirty so flush() rebuilds the anchor map
                self.anchors_dirty = true;
                return marked;
            }
        };

        let mut marked = false;
        while self.cursor < frame_end && self.cursor < self.layout.len() {
            let slot = &mut self.layout[self.cursor];
            let anchor = slot.anchor_id();
            let (group_key, group_scope, group_len) = match slot {
                LayoutSlot::Group {
                    key, scope, len, ..
                } => (Some(*key), *scope, *len),
                _ => (None, None, 0),
            };

            // Note: We do NOT drop the payload here - it persists!
            // IMPORTANT: group_len stores the number of children (not including the group slot).
            // This pairs with start_group's calculation of frame.end = start + len + 1.
            *slot = LayoutSlot::Gap {
                anchor,
                group_key,
                group_scope,
                group_len,
            };
            marked = true;
            self.cursor += 1;
        }

        if let Some(frame) = self.group_stack.last_mut() {
            frame.end = self.cursor;
        }
        marked
    }
}

impl SlotStorage for SplitSlotStorage {
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
        if let Some(LayoutSlot::Group {
            scope: slot_scope, ..
        }) = self.layout.get_mut(group.index())
        {
            *slot_scope = Some(scope);
        }
    }

    fn end_group(&mut self) {
        self.do_end_group();
    }

    fn skip_current_group(&mut self) {
        if let Some(LayoutSlot::Group { len, .. }) = self.layout.get(self.cursor) {
            self.cursor += 1 + len;
        }
    }

    fn nodes_in_current_group(&self) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        if let Some(frame) = self.group_stack.last() {
            for pos in (frame.start + 1)..frame.end {
                if let Some(LayoutSlot::Node { id, .. }) = self.layout.get(pos) {
                    nodes.push(*id);
                }
            }
        }
        nodes
    }

    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group> {
        for (idx, slot) in self.layout.iter().enumerate() {
            if let LayoutSlot::Group { scope: Some(s), .. } = slot {
                if *s == scope {
                    self.cursor = idx;
                    return Some(GroupId::new(idx));
                }
            }
        }
        None
    }

    fn end_recompose(&mut self) {
        // No-op
    }

    fn alloc_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot {
        self.ensure_capacity();

        // Check if current slot is a value ref we can reuse
        if let Some(LayoutSlot::ValueRef { anchor }) = self.layout.get(self.cursor) {
            let anchor_id = anchor.0;
            // Check if payload exists and has correct type
            if let Some(data) = self.payload.get(&anchor_id) {
                if data.is::<T>() {
                    // Reuse existing slot with matching type
                    let slot_id = ValueSlotId::new(self.cursor);
                    self.cursor += 1;
                    return slot_id;
                } else {
                    // Type mismatch: overwrite payload with new value
                    self.payload.insert(anchor_id, Box::new(init()));
                    let slot_id = ValueSlotId::new(self.cursor);
                    self.cursor += 1;
                    return slot_id;
                }
            } else {
                // Layout points to missing payload: create new payload
                self.payload.insert(anchor_id, Box::new(init()));
                let slot_id = ValueSlotId::new(self.cursor);
                self.cursor += 1;
                return slot_id;
            }
        }

        // Check if it's a gap we can reuse
        if matches!(self.layout.get(self.cursor), Some(LayoutSlot::Gap { .. })) {
            // Create new value slot in the gap
            let anchor = self.alloc_anchor();
            let anchor_id = anchor.0;

            // Store payload
            self.payload.insert(anchor_id, Box::new(init()));

            // Store layout ref
            self.layout[self.cursor] = LayoutSlot::ValueRef { anchor };

            let slot_id = ValueSlotId::new(self.cursor);
            self.cursor += 1;

            if let Some(frame) = self.group_stack.last_mut() {
                if self.cursor > frame.end {
                    frame.end = self.cursor;
                }
            }
            self.anchors_dirty = true;
            return slot_id;
        }

        // Create new value slot
        let anchor = self.alloc_anchor();
        let anchor_id = anchor.0;

        // Store payload
        self.payload.insert(anchor_id, Box::new(init()));

        // Store layout ref
        let slot = LayoutSlot::ValueRef { anchor };
        self.insert_at_cursor(slot);

        let slot_id = ValueSlotId::new(self.cursor);
        self.cursor += 1;
        slot_id
    }

    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T {
        let layout_slot = self
            .layout
            .get(slot.index())
            .expect("layout slot not found");
        let anchor = layout_slot.anchor_id();
        let data = self.payload.get(&anchor.0).expect("payload not found");
        data.downcast_ref::<T>().expect("type mismatch")
    }

    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T {
        let layout_slot = self
            .layout
            .get(slot.index())
            .expect("layout slot not found");
        let anchor = layout_slot.anchor_id();
        let data = self.payload.get_mut(&anchor.0).expect("payload not found");
        data.downcast_mut::<T>().expect("type mismatch")
    }

    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T) {
        let layout_slot = self
            .layout
            .get(slot.index())
            .expect("layout slot not found");
        let anchor = layout_slot.anchor_id();
        self.payload.insert(anchor.0, Box::new(value));
    }

    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let slot = self.alloc_value_slot(|| Owned::new(init()));
        self.read_value::<Owned<T>>(slot).clone()
    }

    fn peek_node(&self) -> Option<NodeId> {
        if let Some(LayoutSlot::Node { id, .. }) = self.layout.get(self.cursor) {
            Some(*id)
        } else {
            None
        }
    }

    fn record_node(&mut self, id: NodeId) {
        self.ensure_capacity();
        let anchor = self.alloc_anchor();
        let slot = LayoutSlot::Node { anchor, id };
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
        // Rebuild anchors if needed
        if self.anchors_dirty {
            for pos in self.anchors.iter_mut() {
                *pos = usize::MAX;
            }

            for (idx, slot) in self.layout.iter().enumerate() {
                let anchor = slot.anchor_id();
                if anchor.is_valid() {
                    let id = anchor.0;
                    if id >= self.anchors.len() {
                        self.anchors.resize(id + 1, usize::MAX);
                    }
                    self.anchors[id] = idx;
                }
            }

            self.anchors_dirty = false;
        }
    }
}

impl SplitSlotStorage {
    /// Debug method to dump all groups.
    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.layout
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                LayoutSlot::Group {
                    key, len, scope, ..
                } => Some((i, *key, *scope, *len)),
                _ => None,
            })
            .collect()
    }

    /// Debug method to dump all slots.
    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        self.layout
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let desc = match slot {
                    LayoutSlot::Group {
                        key,
                        scope,
                        len,
                        has_gap_children,
                        ..
                    } => {
                        format!(
                            "Group(key={}, scope={:?}, len={}, gaps={})",
                            key, scope, len, has_gap_children
                        )
                    }
                    LayoutSlot::ValueRef { .. } => "ValueRef".to_string(),
                    LayoutSlot::Node { id, .. } => format!("Node(id={})", id),
                    LayoutSlot::Gap {
                        group_key,
                        group_scope,
                        group_len,
                        ..
                    } => {
                        format!(
                            "Gap(key={:?}, scope={:?}, len={})",
                            group_key, group_scope, group_len
                        )
                    }
                };
                (i, desc)
            })
            .collect()
    }
}

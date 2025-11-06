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
    /// Debug: instance ID for tracking
    instance_id: usize,
    /// Tracks if we're currently in recompose mode (after begin_recompose_at_scope)
    in_recompose_mode: bool,
}

struct GroupFrame {
    key: Key,
    start: usize,
    end: usize,
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
        use std::sync::atomic::{AtomicUsize, Ordering};
        static INSTANCE_COUNTER: AtomicUsize = AtomicUsize::new(0);
        let instance_id = INSTANCE_COUNTER.fetch_add(1, Ordering::SeqCst);
        eprintln!("[SplitSlotStorage::new] Creating instance #{}", instance_id);
        Self {
            next_anchor_id: Cell::new(1), // Start at 1 (0 is INVALID)
            instance_id,
            in_recompose_mode: false,
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

        eprintln!("[insert_at_cursor] cursor={}, inserting: {:?}", self.cursor, match &slot {
            LayoutSlot::Group { key, .. } => format!("Group(key={})", key),
            LayoutSlot::Gap { .. } => "Gap".to_string(),
            LayoutSlot::ValueRef { anchor } => format!("ValueRef(anchor={})", anchor.0),
            LayoutSlot::Node { .. } => "Node".to_string(),
        });

        // Check if we can reuse a gap
        if matches!(self.layout.get(self.cursor), Some(LayoutSlot::Gap { .. })) {
            eprintln!("[insert_at_cursor] Replacing gap at cursor={}", self.cursor);
            self.layout[self.cursor] = slot;
        } else {
            // Need to insert - for simplicity, just overwrite
            // A full implementation would shift
            if self.cursor < self.layout.len() {
                // If we're overwriting a Group, mark its children as gaps first
                if let Some(LayoutSlot::Group { len, .. }) = self.layout.get(self.cursor) {
                    let group_len = *len;
                    if group_len > 0 {
                        // Mark children as gaps
                        let children_start = self.cursor + 1;
                        let children_end = (children_start + group_len).min(self.layout.len());
                        for i in children_start..children_end {
                            if let Some(child_slot) = self.layout.get_mut(i) {
                                // Preserve anchor for all slot types so values can be restored
                                let child_anchor = child_slot.anchor_id();
                                let (child_key, child_scope, child_len) = match child_slot {
                                    LayoutSlot::Group { key, scope, len, .. } => (Some(*key), *scope, *len),
                                    _ => (None, None, 0),
                                };
                                *child_slot = LayoutSlot::Gap {
                                    anchor: child_anchor,
                                    group_key: child_key,
                                    group_scope: child_scope,
                                    group_len: child_len,
                                };
                            }
                        }
                    }
                }
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

        eprintln!("[start_group] instance_id={}, cursor={}, key={}", self.instance_id, self.cursor, key);
        if let Some(slot) = self.layout.get(self.cursor) {
            eprintln!("[start_group] found: {:?}", match slot {
                LayoutSlot::Group { key, .. } => format!("Group(key={})", key),
                LayoutSlot::Gap { group_key, .. } => format!("Gap(group_key={:?})", group_key),
                LayoutSlot::ValueRef { anchor } => format!("ValueRef(anchor={})", anchor.0),
                LayoutSlot::Node { .. } => "Node".to_string(),
            });
        }

        // Check if parent is forcing children to recompose
        let parent_force = self.group_stack.last().map_or(false, |frame| frame.force_children_recompose);

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
                // Convert Gap back to Group
                self.layout[self.cursor] = LayoutSlot::Group {
                    key,
                    anchor,
                    len,
                    scope,
                    has_gap_children: true, // Mark that children need recreation
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
                    force_children_recompose: true, // Force recreation of child values on first pass
                });
                self.last_start_was_gap = true;
                self.anchors_dirty = true;
                return (start, true);
            }
        }

        // Check if we're reusing an existing group (not from gap)
        if let Some(LayoutSlot::Group {
            key: existing_key,
            has_gap_children,
            len,
            ..
        }) = self.layout.get(self.cursor)
        {
            if *existing_key == key {
                let had_gap_children = *has_gap_children;
                let len = *len;
                let start = self.cursor;

                // Clear the flag now that we're entering the group
                if let Some(LayoutSlot::Group {
                    has_gap_children: flag,
                    ..
                }) = self.layout.get_mut(self.cursor)
                {
                    *flag = false;
                }

                self.cursor += 1;
                // Only force children recompose if this was the first entry after gap restoration
                let force_children = had_gap_children || parent_force;
                self.group_stack.push(GroupFrame {
                    key,
                    start,
                    end: start + len + 1,
                    force_children_recompose: force_children,
                });
                self.last_start_was_gap = false;
                return (start, false);
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
        // Propagate force_children_recompose from parent
        self.group_stack.push(GroupFrame {
            key,
            start,
            end: start,
            force_children_recompose: parent_force,
        });
        self.last_start_was_gap = false;
        (start, false)
    }

    fn do_end_group(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            let end = self.cursor;
            let len = end.saturating_sub(frame.start + 1);
            if let Some(LayoutSlot::Group { len: slot_len, has_gap_children, .. }) = self.layout.get_mut(frame.start)
            {
                let old_len = *slot_len;
                *slot_len = len;
                // If the group shrunk, mark that it has gap children
                if len < old_len {
                    *has_gap_children = true;
                }
            }
        }
    }

    fn do_finalize_current_group(&mut self) -> bool {
        let frame_end = match self.group_stack.last() {
            Some(frame) => frame.end,
            None => {
                eprintln!("[finalize] ROOT level: cursor={}, layout.len()={}", self.cursor, self.layout.len());
                // Root-level finalization: mark everything from cursor to end as gaps
                if self.cursor >= self.layout.len() {
                    return false;
                }
                let mut marked = false;
                while self.cursor < self.layout.len() {
                    let slot = &mut self.layout[self.cursor];
                    let anchor = slot.anchor_id();
                    let (group_key, group_scope, group_len) = match slot {
                        LayoutSlot::Group { key, scope, len, .. } => {
                            eprintln!("[finalize] ROOT marking Group at {} (key={}) as Gap", self.cursor, key);
                            (Some(*key), *scope, *len)
                        },
                        LayoutSlot::ValueRef { .. } => {
                            eprintln!("[finalize] ROOT marking ValueRef at {} (anchor={}) as Gap", self.cursor, anchor.0);
                            (None, None, 0)
                        },
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

        eprintln!("[finalize] IN-GROUP: cursor={}, frame_end={}", self.cursor, frame_end);
        let mut marked = false;
        while self.cursor < frame_end && self.cursor < self.layout.len() {
            let slot = &mut self.layout[self.cursor];
            let anchor = slot.anchor_id();
            let (group_key, group_scope, group_len) = match slot {
                LayoutSlot::Group { key, scope, len, .. } => {
                    eprintln!("[finalize] IN-GROUP marking Group at {} (key={}) as Gap", self.cursor, key);
                    (Some(*key), *scope, *len)
                },
                LayoutSlot::ValueRef { .. } => {
                    eprintln!("[finalize] IN-GROUP marking ValueRef at {} (anchor={}) as Gap", self.cursor, anchor.0);
                    (None, None, 0)
                },
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
        eprintln!("[begin_recompose_at_scope] instance_id={}, searching for scope={}", self.instance_id, scope);
        for (idx, slot) in self.layout.iter().enumerate() {
            if let LayoutSlot::Group {
                scope: Some(s),
                key,
                len,
                has_gap_children,
                ..
            } = slot
            {
                if *s == scope {
                    eprintln!("[begin_recompose_at_scope] Found scope {} at idx={}, entering group", scope, idx);
                    // Enter the group and skip RecomposeScope
                    self.cursor = idx + 1;

                    // Skip RecomposeScope if present
                    if let Some(LayoutSlot::ValueRef { anchor }) = self.layout.get(self.cursor) {
                        if let Some(data) = self.payload.get(&anchor.0) {
                            if (**data).type_id() == std::any::TypeId::of::<crate::owned::Owned<crate::RecomposeScope>>() {
                                eprintln!("[begin_recompose_at_scope] Skipping RecomposeScope at cursor={}", self.cursor);
                                self.cursor += 1;
                            }
                        }
                    }

                    self.in_recompose_mode = true;
                    return Some(GroupId::new(idx));
                }
            }
        }
        eprintln!("[begin_recompose_at_scope] Scope {} NOT FOUND", scope);
        None
    }

    fn end_recompose(&mut self) {
        self.in_recompose_mode = false;
    }

    fn alloc_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot {
        self.ensure_capacity();

        // NOTE: force_children_recompose is ONLY for child GROUPS, not for value slots!
        // Value slots should always be reused if the type matches, just like in Baseline.

        eprintln!("[alloc_value_slot] cursor={}, type={}", self.cursor, std::any::type_name::<T>());
        if let Some(slot) = self.layout.get(self.cursor) {
            eprintln!("[alloc_value_slot] found slot: {:?}", match slot {
                LayoutSlot::ValueRef { anchor } => format!("ValueRef(anchor={})", anchor.0),
                LayoutSlot::Gap { anchor, group_key, .. } => format!("Gap(anchor={}, is_group={})", anchor.0, group_key.is_some()),
                LayoutSlot::Group { .. } => "Group".to_string(),
                LayoutSlot::Node { .. } => "Node".to_string(),
            });
        }

        // Check if current slot is a value ref we can reuse
        if let Some(LayoutSlot::ValueRef { anchor }) = self.layout.get(self.cursor) {
            let anchor_id = anchor.0;
            eprintln!("[alloc_value_slot] Found ValueRef with anchor={}", anchor_id);
            // Check if payload exists and has correct type
            if let Some(data) = self.payload.get(&anchor_id) {
                eprintln!("[alloc_value_slot] Payload exists, type matches: {}", data.is::<T>());
                if data.is::<T>() {
                    eprintln!("[alloc_value_slot] ✓ Reusing existing ValueRef");
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
        if let Some(LayoutSlot::Gap { anchor, group_key, .. }) = self.layout.get(self.cursor) {
            let gap_anchor = *anchor;
            let is_group_gap = group_key.is_some();

            eprintln!("[alloc_value_slot] Gap: anchor={}, is_valid={}, is_group={}", gap_anchor.0, gap_anchor.is_valid(), is_group_gap);

            // If this gap was NOT a group (i.e., it was a value or node slot),
            // try to restore it as a ValueRef and reuse the payload
            if !is_group_gap && gap_anchor.is_valid() {
                let anchor_id = gap_anchor.0;
                // Check if payload exists and has correct type
                if let Some(data) = self.payload.get(&anchor_id) {
                    eprintln!("[alloc_value_slot] Gap has payload, type matches: {}", data.is::<T>());
                    if data.is::<T>() {
                        eprintln!("[alloc_value_slot] ✓ Restoring Gap as ValueRef with existing payload");
                        // Restore the ValueRef and reuse existing payload
                        self.layout[self.cursor] = LayoutSlot::ValueRef { anchor: gap_anchor };
                        let slot_id = ValueSlotId::new(self.cursor);
                        self.cursor += 1;
                        self.anchors_dirty = true;
                        return slot_id;
                    } else {
                        // Type mismatch: overwrite payload with new value
                        self.payload.insert(anchor_id, Box::new(init()));
                        self.layout[self.cursor] = LayoutSlot::ValueRef { anchor: gap_anchor };
                        let slot_id = ValueSlotId::new(self.cursor);
                        self.cursor += 1;
                        self.anchors_dirty = true;
                        return slot_id;
                    }
                }
            }

            // Otherwise, create new value slot in the gap
            let anchor = self.alloc_anchor();
            let anchor_id = anchor.0;

            eprintln!("[alloc_value_slot] Creating new ValueRef in gap at cursor={} with anchor={}, type={}", self.cursor, anchor_id, std::any::type_name::<T>());

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

        eprintln!("[alloc_value_slot] Creating new ValueRef at cursor={} with anchor={}, type={}", self.cursor, anchor_id, std::any::type_name::<T>());

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
        let layout_slot = self.layout.get(slot.index()).expect("layout slot not found");
        let anchor = layout_slot.anchor_id();
        let data = self.payload.get(&anchor.0).expect("payload not found");
        data.downcast_ref::<T>().expect("type mismatch")
    }

    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T {
        let layout_slot = self.layout.get(slot.index()).expect("layout slot not found");
        let anchor = layout_slot.anchor_id();
        let data = self.payload.get_mut(&anchor.0).expect("payload not found");
        data.downcast_mut::<T>().expect("type mismatch")
    }

    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T) {
        let layout_slot = self.layout.get(slot.index()).expect("layout slot not found");
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
        eprintln!("[reset] instance_id={}, layout.len()={}, first few slots:", self.instance_id, self.layout.len());
        for (i, slot) in self.layout.iter().take(10).enumerate() {
            let desc = match slot {
                LayoutSlot::Group { key, len, .. } => format!("Group(key={}, len={})", key, len),
                LayoutSlot::Gap { anchor, group_key, .. } => format!("Gap(anchor={}, is_group={})", anchor.0, group_key.is_some()),
                LayoutSlot::ValueRef { anchor } => format!("ValueRef(anchor={})", anchor.0),
                LayoutSlot::Node { .. } => "Node".to_string(),
            };
            eprintln!("  [{}] {}", i, desc);
        }
        self.cursor = 0;
        self.group_stack.clear();
        eprintln!("[reset] cursor reset to 0");
    }

    fn flush(&mut self) {
        eprintln!("[flush] instance_id={}, layout.len()={}, cursor={}", self.instance_id, self.layout.len(), self.cursor);
        eprintln!("[flush] Slots 15-25:");
        for i in 15..25.min(self.layout.len()) {
            if let Some(slot) = self.layout.get(i) {
                let desc = match slot {
                    LayoutSlot::Group { key, len, scope, .. } => format!("Group(key={}, len={}, scope={:?})", key, len, scope),
                    LayoutSlot::Gap { anchor, group_key, .. } => format!("Gap(anchor={}, is_group={})", anchor.0, group_key.is_some()),
                    LayoutSlot::ValueRef { anchor } => format!("ValueRef(anchor={})", anchor.0),
                    LayoutSlot::Node { .. } => "Node".to_string(),
                };
                eprintln!("  [{}] {}", i, desc);
            }
        }
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
                LayoutSlot::Group { key, len, scope, .. } => Some((i, *key, *scope, *len)),
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
                    LayoutSlot::Group { key, scope, len, has_gap_children, .. } => {
                        format!("Group(key={}, scope={:?}, len={}, gaps={})", key, scope, len, has_gap_children)
                    }
                    LayoutSlot::ValueRef { .. } => "ValueRef".to_string(),
                    LayoutSlot::Node { id, .. } => format!("Node(id={})", id),
                    LayoutSlot::Gap { group_key, group_scope, group_len, .. } => {
                        format!("Gap(key={:?}, scope={:?}, len={})", group_key, group_scope, group_len)
                    }
                };
                (i, desc)
            })
            .collect()
    }
}

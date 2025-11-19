//! Slot table implementation using a gap-buffer strategy.
//!
//! This is the baseline/reference slot storage implementation that provides:
//! - Gap-based slot reuse during conditional rendering
//! - Anchor-based positional stability during reorganization
//! - Efficient group skipping and scope-based recomposition
//! - Batch anchor rebuilding for large structural changes

use crate::{
    slot_storage::{GroupId, SlotStorage, StartGroup, ValueSlotId},
    AnchorId, Key, NodeId, Owned, ScopeId,
};
use std::any::Any;
use std::cell::Cell;

#[derive(Default)]
pub struct SlotTable {
    slots: Vec<Slot>, // FUTURE(no_std): replace Vec with arena-backed slot storage.
    cursor: usize,
    group_stack: Vec<GroupFrame>, // FUTURE(no_std): switch to small stack buffer.
    /// Maps anchor IDs to their current physical positions in the slots array.
    /// This indirection layer provides positional stability during slot reorganization.
    anchors: Vec<usize>, // index = anchor_id.0
    anchors_dirty: bool,
    /// Counter for allocating unique anchor IDs.
    next_anchor_id: Cell<usize>,
    /// Tracks whether the most recent start() reused a gap slot.
    last_start_was_gap: bool,
}

enum Slot {
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
    /// Gap: Marks an unused slot that can be reused or compacted.
    /// This prevents destructive truncation that would destroy sibling components.
    /// For Groups marked as gaps (e.g., during tab switching), we preserve their
    /// key, scope, and length so they can be properly matched and reused when reactivated.
    Gap {
        anchor: AnchorId,
        /// If this gap was a Group, preserve its key for reuse matching
        group_key: Option<Key>,
        /// If this gap was a Group, preserve its scope ID for state subscription continuity
        group_scope: Option<ScopeId>,
        /// If this gap was a Group, preserve its length to restore the full group structure
        group_len: usize,
    },
}

struct GroupFrame {
    key: Key,
    start: usize, // Physical position (will be phased out)
    end: usize,   // Physical position (will be phased out)
    force_children_recompose: bool,
}

const INVALID_ANCHOR_POS: usize = usize::MAX;

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
            Slot::Node { .. } => SlotKind::Node,
            Slot::Gap { .. } => SlotKind::Gap,
        }
    }

    /// Get the anchor ID for this slot.
    fn anchor_id(&self) -> AnchorId {
        match self {
            Slot::Group { anchor, .. } => *anchor,
            Slot::Value { anchor, .. } => *anchor,
            Slot::Node { anchor, .. } => *anchor,
            Slot::Gap { anchor, .. } => *anchor,
        }
    }

    fn as_value<T: 'static>(&self) -> &T {
        match self {
            Slot::Value { data, .. } => data.downcast_ref::<T>().expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }

    fn as_value_mut<T: 'static>(&mut self) -> &mut T {
        match self {
            Slot::Value { data, .. } => data.downcast_mut::<T>().expect("slot value type mismatch"),
            _ => panic!("slot is not a value"),
        }
    }
}

impl Default for Slot {
    fn default() -> Self {
        Slot::Group {
            key: 0,
            anchor: AnchorId::INVALID,
            len: 0,
            scope: None,
            has_gap_children: false,
        }
    }
}

impl SlotTable {
    const INITIAL_CAP: usize = 32;
    #[allow(dead_code)]
    const GAP_BLOCK: usize = 32; // tune 16/32/64
    const LOCAL_GAP_SCAN: usize = 256; // tune

    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            cursor: 0,
            group_stack: Vec::new(),
            anchors: Vec::new(),
            anchors_dirty: false,
            next_anchor_id: Cell::new(1), // Start at 1 (0 is INVALID)
            last_start_was_gap: false,
        }
    }

    fn ensure_capacity(&mut self) {
        if self.slots.is_empty() {
            self.slots.reserve(Self::INITIAL_CAP);
            self.append_gap_slots(Self::INITIAL_CAP);
        } else if self.cursor == self.slots.len() {
            self.grow_slots();
        }
    }

    /// Ensure that at `cursor` there is at least 1 gap slot.
    /// We do this by pulling a small block of gap slots from the tail forward,
    /// shifting everything in between once, and fixing frames/anchors.
    #[allow(dead_code)]
    fn ensure_gap_at(&mut self, cursor: usize) {
        // if already a gap, nothing to do
        if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
            return;
        }

        // make sure we actually have tail gaps to steal from
        self.ensure_capacity(); // <- your existing one, will append gaps at the end
                                // after this, the last N slots are guaranteed to be gaps with INVALID anchors

        loop {
            // how many gaps we want to pull forward
            let mut tail_start = self.slots.len();
            let mut block = 0usize;

            while tail_start > cursor && block < Self::GAP_BLOCK {
                match self.slots.get(tail_start - 1) {
                    Some(Slot::Gap { anchor, .. }) if *anchor == AnchorId::INVALID => {
                        tail_start -= 1;
                        block += 1;
                    }
                    _ => break,
                }
            }

            if block == 0 {
                // no tail gaps yet, grow and try again
                self.grow_slots();
                continue;
            }

            if tail_start > cursor {
                // 1) shift group frames and anchors for the slice [cursor..tail_start)
                //    because we're about to move it right by `block`.
                self.shift_group_frames(cursor, block as isize);
                self.shift_anchor_positions_from(cursor, block as isize);
            }

            // 2) actually move the slice up in the Vec
            //
            // we currently have:
            //   [ ... cursor | ...... live slots ...... | gap gap gap gap ]
            // we want:
            //   [ ... cursor | gap gap gap gap | ...... live slots ...... ]
            //
            // we can do that with rotate_right because we already reserved space.
            const MAX_ROTATE_WINDOW: usize = 4096; // tune

            let end = tail_start + block;
            let win = end - cursor;
            if win > MAX_ROTATE_WINDOW {
                // too expensive to pull from the tail — just grow and put a gap right here
                // or fall back to “overwrite here” logic
                self.force_gap_here(cursor);
                return;
            }

            let end = tail_start + block;
            self.slots[cursor..end].rotate_right(block);

            // 3) now fill [cursor .. cursor+block) with fresh gaps
            for i in 0..block {
                self.slots[cursor + i] = Slot::Gap {
                    anchor: AnchorId::INVALID,
                    group_key: None,
                    group_scope: None,
                    group_len: 0,
                };
            }
            // done: cursor is guaranteed to be a gap now
            break;
        }
    }
    fn force_gap_here(&mut self, cursor: usize) {
        // we *know* we have capacity (ensure_capacity() already ran)
        // so just overwrite the slot at cursor with a fresh gap
        self.slots[cursor] = Slot::Gap {
            anchor: AnchorId::INVALID,
            group_key: None,
            group_scope: None,
            group_len: 0,
        };
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

    fn ensure_gap_at_local(&mut self, cursor: usize) {
        if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
            return;
        }
        self.ensure_capacity();

        if let Some((run_start, run_len)) = self.find_right_gap_run(cursor, Self::LOCAL_GAP_SCAN) {
            self.shift_group_frames(cursor, run_len as isize);
            self.shift_anchor_positions_from(cursor, run_len as isize);
            self.slots[cursor..run_start + run_len].rotate_right(run_len);
            return;
        }

        self.force_gap_here(cursor);
    }

    fn append_gap_slots(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        for _ in 0..count {
            self.slots.push(Slot::Gap {
                anchor: AnchorId::INVALID,
                group_key: None,
                group_scope: None,
                group_len: 0,
            });
        }
    }

    fn grow_slots(&mut self) {
        let old_len = self.slots.len();
        let target_len = (old_len.saturating_mul(2)).max(Self::INITIAL_CAP);
        let additional = target_len.saturating_sub(old_len);
        if additional == 0 {
            return;
        }
        self.slots.reserve(additional);
        self.append_gap_slots(additional);
    }

    /// Allocate a new unique anchor ID.
    fn allocate_anchor(&self) -> AnchorId {
        let id = self.next_anchor_id.get();
        self.next_anchor_id.set(id + 1);
        AnchorId(id)
    }

    /// Register an anchor at a specific position in the slots array.
    fn register_anchor(&mut self, anchor: AnchorId, position: usize) {
        debug_assert!(anchor.is_valid(), "attempted to register invalid anchor");
        let idx = anchor.0;
        if idx == 0 {
            return;
        }
        if idx >= self.anchors.len() {
            self.anchors.resize(idx + 1, INVALID_ANCHOR_POS);
        }
        self.anchors[idx] = position;
    }

    /// Returns whether the most recent `start` invocation reused a gap slot.
    /// Resets the flag to false after reading.
    fn take_last_start_was_gap(&mut self) -> bool {
        let was_gap = self.last_start_was_gap;
        self.last_start_was_gap = false;
        was_gap
    }

    fn find_group_owner_index(&self, start: usize) -> Option<usize> {
        if start == 0 || self.slots.is_empty() {
            return None;
        }

        let mut idx = start.saturating_sub(1);
        loop {
            if let Some(Slot::Group { len, .. }) = self.slots.get(idx) {
                let extent_end = idx.saturating_add(*len);
                if start < extent_end {
                    return Some(idx);
                }
            }
            if idx == 0 {
                break;
            }
            idx -= 1;
        }
        None
    }

    /// Resolve an anchor to its current position in the slots array.
    fn resolve_anchor(&self, anchor: AnchorId) -> Option<usize> {
        let idx = anchor.0;
        if idx == 0 {
            return None;
        }
        self.anchors
            .get(idx)
            .copied()
            .filter(|&pos| pos != INVALID_ANCHOR_POS)
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
        let mut i = start;
        let end = end.min(self.slots.len());
        let mut marked_any = false;

        while i < end {
            if i >= self.slots.len() {
                break;
            }

            let (anchor, group_len, group_key, group_scope) = {
                let slot = &self.slots[i];
                let anchor = slot.anchor_id();
                let (group_len, group_key, group_scope) = match slot {
                    Slot::Group {
                        len, key, scope, ..
                    } => (*len, Some(*key), *scope),
                    // Also preserve metadata for existing Gaps!
                    // This is essential for the decrease-increase scenario where gaps
                    // are re-marked as gaps but need to retain their original keys.
                    Slot::Gap {
                        group_len,
                        group_key,
                        group_scope,
                        ..
                    } => (*group_len, *group_key, *group_scope),
                    _ => (0, None, None),
                };
                (anchor, group_len, group_key, group_scope)
            };

            // Mark this slot as a gap, preserving Group metadata if it was a Group
            // This allows Groups to be properly matched and reused during tab switching
            self.slots[i] = Slot::Gap {
                anchor,
                group_key,
                group_scope,
                group_len,
            };
            marked_any = true;

            // If it was a group, recursively mark its children as gaps too
            if group_len > 0 {
                // Mark children (from i+1 to i+group_len)
                let children_end = (i + group_len).min(end);
                for j in (i + 1)..children_end {
                    if j < self.slots.len() {
                        // For nested Groups, preserve their metadata as well
                        if let Slot::Group {
                            key: nested_key,
                            scope: nested_scope,
                            len: nested_len,
                            ..
                        } = self.slots[j]
                        {
                            let child_anchor = self.slots[j].anchor_id();
                            self.slots[j] = Slot::Gap {
                                anchor: child_anchor,
                                group_key: Some(nested_key),
                                group_scope: nested_scope,
                                group_len: nested_len,
                            };
                            marked_any = true;
                        } else {
                            // For Nodes and other slots, mark as regular gaps
                            let child_anchor = self.slots[j].anchor_id();
                            self.slots[j] = Slot::Gap {
                                anchor: child_anchor,
                                group_key: None,
                                group_scope: None,
                                group_len: 0,
                            };
                            marked_any = true;
                        }
                    }
                }
                i = (i + group_len).max(i + 1);
            } else {
                i += 1;
            }
        }

        if marked_any {
            let owner_idx = owner_index.or_else(|| self.find_group_owner_index(start));
            if let Some(idx) = owner_idx {
                if let Some(Slot::Group {
                    has_gap_children, ..
                }) = self.slots.get_mut(idx)
                {
                    *has_gap_children = true;
                }
                if let Some(frame) = self.group_stack.iter_mut().find(|frame| frame.start == idx) {
                    frame.force_children_recompose = true;
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
            Slot::Group { scope, .. } => *scope,
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
                *scope_opt = Some(scope);
            }
            _ => panic!("set_group_scope: slot at index is not a group"),
        }
    }

    pub fn find_group_index_by_scope(&self, scope: ScopeId) -> Option<usize> {
        self.slots
            .iter()
            .enumerate()
            .find_map(|(i, slot)| match slot {
                Slot::Group {
                    scope: Some(id), ..
                } if *id == scope => Some(i),
                _ => None,
            })
    }

    pub fn start_recompose_at_scope(&mut self, scope: ScopeId) -> Option<usize> {
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
                } => Some((i, *key, *scope, *len)),
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
                    } => format!("Group(key={:?}, scope={:?}, len={})", key, scope, len),
                    Slot::Value { .. } => "Value".to_string(),
                    Slot::Node { id, .. } => format!("Node(id={:?})", id),
                    Slot::Gap {
                        group_key,
                        group_scope,
                        ..
                    } => {
                        if let Some(key) = group_key {
                            format!("Gap(was_group_key={:?}, scope={:?})", key, group_scope)
                        } else {
                            "Gap".to_string()
                        }
                    }
                };
                (i, kind)
            })
            .collect()
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
        let mut max_anchor = 0usize;
        for slot in &self.slots {
            let idx = slot.anchor_id().0;
            if idx > max_anchor {
                max_anchor = idx;
            }
        }
        if self.anchors.len() <= max_anchor {
            self.anchors.resize(max_anchor + 1, INVALID_ANCHOR_POS);
        }

        for pos in &mut self.anchors {
            *pos = INVALID_ANCHOR_POS;
        }

        for (position, slot) in self.slots.iter().enumerate() {
            let idx = slot.anchor_id().0;
            if idx == 0 {
                continue;
            }
            self.anchors[idx] = position;
        }
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
        let parent_force = self
            .group_stack
            .last()
            .map(|frame| frame.force_children_recompose)
            .unwrap_or(false);

        // === FAST PATH =======================================================
        if let Some(Slot::Group {
            key: existing_key,
            len,
            has_gap_children,
            ..
        }) = self.slots.get(cursor)
        {
            // Only fast-path if:
            // 1) key matches
            // 2) there were NO gap children before
            // 3) parent is NOT forcing children to recompose
            if *existing_key == key && !*has_gap_children && !parent_force {
                self.last_start_was_gap = false;

                let frame = GroupFrame {
                    key,
                    start: cursor,
                    end: cursor + *len,
                    force_children_recompose: false,
                };
                self.group_stack.push(frame);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }
        }

        // if parent says "my children are unstable", don't try to be clever
        if parent_force {
            if let Some(Slot::Group {
                key: existing_key,
                len,
                has_gap_children,
                ..
            }) = self.slots.get_mut(cursor)
            {
                if *existing_key == key {
                    *has_gap_children = false;
                    self.last_start_was_gap = true;
                    let frame = GroupFrame {
                        key,
                        start: cursor,
                        end: cursor + *len,
                        force_children_recompose: true,
                    };
                    self.group_stack.push(frame);
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
                    return cursor;
                }
            }

            if let Some(Slot::Gap {
                anchor,
                group_key: Some(gap_key),
                group_scope,
                group_len,
            }) = self.slots.get(cursor)
            {
                if *gap_key == key {
                    let anchor = *anchor;
                    let gap_len = *group_len;
                    let preserved_scope = *group_scope;
                    self.slots[cursor] = Slot::Group {
                        key,
                        anchor,
                        len: gap_len,
                        scope: preserved_scope,
                        has_gap_children: false,
                    };
                    self.register_anchor(anchor, cursor);
                    self.last_start_was_gap = true;
                    let frame = GroupFrame {
                        key,
                        start: cursor,
                        end: cursor + gap_len,
                        force_children_recompose: true,
                    };
                    self.group_stack.push(frame);
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
                    return cursor;
                }
            }

            return self.insert_new_group_at_cursor(key);
        }

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
                    return self.insert_new_group_at_cursor(key);
                }
            }
        }

        // Check if we can reuse an existing Group at the cursor position without scanning.
        if let Some(slot) = self.slots.get_mut(cursor) {
            if let Slot::Group {
                key: existing_key,
                len,
                has_gap_children,
                ..
            } = slot
            {
                if *existing_key == key {
                    let group_len = *len;
                    let had_gap_children = *has_gap_children;
                    if had_gap_children {
                        *has_gap_children = false;
                    }
                    let force_children = had_gap_children || parent_force;

                    self.last_start_was_gap = false;
                    let frame = GroupFrame {
                        key,
                        start: cursor,
                        end: cursor + group_len,
                        force_children_recompose: force_children,
                    };
                    self.group_stack.push(frame);
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
                    return cursor;
                }
            }
        }

        // Check if we can reuse an existing Group/GAP by converting in-place.
        let mut reused_from_gap = false;
        let reuse_result = match self.slots.get(cursor) {
            // If there's already a Group here but with a different key, mark it as a gap
            // instead of recycling it. This preserves the group so it can be found and
            // restored later (critical for the decrease-increase and tab-switching scenarios).
            // Then fall through to the search logic to find the desired key.
            Some(Slot::Group {
                key: existing_key,
                anchor: old_anchor,
                len: old_len,
                scope: old_scope,
                has_gap_children: _,
            }) if *existing_key != key => {
                // Copy all values to avoid borrow checker issues
                let old_key = *existing_key;
                let old_anchor_val = *old_anchor;
                let old_len_val = *old_len;
                let old_scope_val = *old_scope;

                // Mark the group's children as gaps so they can be reused safely
                let group_len = old_len_val.max(1);
                if group_len > 1 {
                    let start = cursor + 1;
                    let end = cursor + group_len;
                    let _ = self.mark_range_as_gaps(start, end, Some(cursor));
                }

                // Mark this group as a gap, preserving all its metadata
                self.slots[cursor] = Slot::Gap {
                    anchor: old_anchor_val,
                    group_key: Some(old_key),
                    group_scope: old_scope_val,
                    group_len: old_len_val,
                };
                // Don't return early - fall through to search logic
                None
            }
            // Also check for Gaps that were Groups with matching keys!
            // This enables tab switching to reuse Groups that were marked as gaps.
            Some(Slot::Gap {
                anchor,
                group_key: Some(gap_key),
                group_scope,
                group_len,
            }) if *gap_key == key => {
                // Convert the Gap back to a Group, preserving its scope and length
                reused_from_gap = true;
                Some((*group_len, *anchor, *group_scope))
            }
            Some(_slot) => None,
            None => None,
        };
        if let Some((len, group_anchor, preserved_scope)) = reuse_result {
            // Convert Gap back to Group if needed
            if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                self.slots[cursor] = Slot::Group {
                    key,
                    anchor: group_anchor,
                    len,
                    scope: preserved_scope,
                    has_gap_children: false,
                };
                self.register_anchor(group_anchor, cursor);
            }

            if reused_from_gap {
                if let Some(Slot::Group {
                    has_gap_children, ..
                }) = self.slots.get_mut(cursor)
                {
                    *has_gap_children = false;
                }
            }

            self.last_start_was_gap = reused_from_gap || parent_force;
            let frame = GroupFrame {
                key,
                start: cursor,
                end: cursor + len,
                force_children_recompose: reused_from_gap || parent_force,
            };
            self.group_stack.push(frame);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return cursor;
        }

        let allow_rescue = !parent_force && cursor < self.slots.len().saturating_sub(1);
        if !allow_rescue {
            return self.insert_new_group_at_cursor(key);
        }

        // When a group is restored from a gap, its preserved length may not accurately
        // reflect where child groups/gaps are now located. To prevent duplicate group
        // creation, we need to search beyond the preserved extent.
        //
        // Strategy: Search up to parent's end first (respects group hierarchy), but
        // if that's less than slots.len(), we'll continue searching in a second pass
        // below to find gaps that should be restored.
        let parent_end = self
            .group_stack
            .last()
            .map(|frame| frame.end.min(self.slots.len()))
            .unwrap_or(self.slots.len());

        // If parent_end seems constrained (less than slots length), we'll do an
        // extended search for gaps after the initial search fails.
        let needs_extended_search = parent_end < self.slots.len();

        let mut search_index = cursor;
        let mut found_group: Option<(usize, AnchorId, usize, Option<ScopeId>)> = None;
        const SEARCH_BUDGET: usize = 16;
        let mut scanned = 0usize;
        while search_index < parent_end && scanned < SEARCH_BUDGET {
            scanned += 1;
            match self.slots.get(search_index) {
                Some(Slot::Group {
                    key: existing_key,
                    anchor,
                    len,
                    scope: _,
                    ..
                }) => {
                    let group_len = *len;
                    if *existing_key == key {
                        found_group = Some((search_index, *anchor, group_len, None));
                        break;
                    }
                    // Search this group's children for gaps with matching keys
                    // IMPORTANT: For gaps, we must search recursively inside them because
                    // gaps can contain nested gaps after multiple decrease-increase cycles
                    let mut child_index = search_index + 1;
                    let search_limit = (search_index + group_len).min(self.slots.len());
                    while child_index < search_limit {
                        match self.slots.get(child_index) {
                            Some(Slot::Gap {
                                anchor: gap_anchor,
                                group_key: Some(gap_key),
                                group_scope,
                                group_len: gap_len,
                            }) if *gap_key == key => {
                                // Found a matching gap!
                                found_group =
                                    Some((child_index, *gap_anchor, *gap_len, *group_scope));
                                break;
                            }
                            Some(Slot::Gap {
                                group_len: gap_len, ..
                            }) => {
                                // Skip this gap - we don't move deeply nested gaps
                                // They will be restored when their parent gap is converted to a group
                                child_index += (*gap_len).max(1);
                            }
                            Some(Slot::Group {
                                len: nested_len, ..
                            }) => {
                                // Skip active groups (don't search inside them)
                                child_index += (*nested_len).max(1);
                            }
                            _ => {
                                child_index += 1;
                            }
                        }
                    }
                    if found_group.is_some() {
                        break;
                    }
                    let advance = group_len.max(1);
                    search_index = search_index.saturating_add(advance);
                }
                // Also search for Gaps that were Groups with matching keys
                Some(Slot::Gap {
                    anchor,
                    group_key: Some(gap_key),
                    group_scope,
                    group_len,
                }) => {
                    if *gap_key == key {
                        found_group = Some((search_index, *anchor, *group_len, *group_scope));
                        break;
                    }
                    // Search this gap's children for gaps with matching keys
                    // IMPORTANT: For nested gaps, search recursively inside them
                    let gap_len_val = *group_len;
                    let mut child_index = search_index + 1;
                    let search_limit = (search_index + gap_len_val).min(self.slots.len());
                    while child_index < search_limit {
                        match self.slots.get(child_index) {
                            Some(Slot::Gap {
                                anchor: child_gap_anchor,
                                group_key: Some(child_gap_key),
                                group_scope: child_group_scope,
                                group_len: child_gap_len,
                            }) if *child_gap_key == key => {
                                // Found a matching gap!
                                found_group = Some((
                                    child_index,
                                    *child_gap_anchor,
                                    *child_gap_len,
                                    *child_group_scope,
                                ));
                                break;
                            }
                            Some(Slot::Gap {
                                group_len: nested_gap_len,
                                ..
                            }) => {
                                // Skip this gap - we don't move deeply nested gaps
                                child_index += (*nested_gap_len).max(1);
                            }
                            Some(Slot::Group {
                                len: nested_len, ..
                            }) => {
                                // Skip active groups (don't search inside them)
                                child_index += (*nested_len).max(1);
                            }
                            _ => {
                                child_index += 1;
                            }
                        }
                    }
                    if found_group.is_some() {
                        break;
                    }
                    let advance = gap_len_val.max(1);
                    search_index = search_index.saturating_add(advance);
                }
                Some(_slot) => {
                    search_index += 1;
                }
                None => break,
            }
        }

        // Extended search: If we didn't find the group within parent_end, but there are
        // more slots to search, look for BOTH groups and gaps with matching keys.
        // This handles the recursive decrease-increase case where a parent gap is restored
        // but its children (which may be groups or gaps) are still at their original
        // positions beyond the parent's preserved extent.
        //
        // IMPORTANT: After multiple decrease-increase cycles, gaps can drift beyond
        // the parent's extent. We search all remaining slots to ensure we find the
        // matching group/gap, regardless of structure complexity.
        if found_group.is_none() && needs_extended_search {
            search_index = parent_end;
            const EXTENDED_SEARCH_BUDGET: usize = 16;
            let mut extended_scanned = 0usize;
            while search_index < self.slots.len() && extended_scanned < EXTENDED_SEARCH_BUDGET {
                extended_scanned += 1;
                match self.slots.get(search_index) {
                    // Search for groups with matching keys (but only within limited range)
                    Some(Slot::Group {
                        key: existing_key,
                        anchor,
                        len,
                        scope,
                        ..
                    }) => {
                        if *existing_key == key {
                            found_group = Some((search_index, *anchor, *len, *scope));
                            break;
                        }
                        let advance = (*len).max(1);
                        search_index = search_index.saturating_add(advance);
                    }
                    // Search for gaps with matching keys (candidates for restoration)
                    Some(Slot::Gap {
                        anchor,
                        group_key: Some(gap_key),
                        group_scope,
                        group_len,
                    }) => {
                        if *gap_key == key {
                            found_group = Some((search_index, *anchor, *group_len, *group_scope));
                            break;
                        }
                        let advance = (*group_len).max(1);
                        search_index = search_index.saturating_add(advance);
                    }
                    Some(_slot) => {
                        search_index += 1;
                    }
                    None => break,
                }
            }
        }

        if let Some((found_index, group_anchor, group_len, preserved_scope)) = found_group {
            // If we found a Gap, convert it back to a Group first
            let reused_gap = matches!(self.slots.get(found_index), Some(Slot::Gap { .. }));
            if reused_gap {
                self.slots[found_index] = Slot::Group {
                    key,
                    anchor: group_anchor,
                    len: group_len,
                    scope: preserved_scope,
                    has_gap_children: false,
                };
            }

            self.last_start_was_gap = reused_gap || parent_force;
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
                // Update all anchor positions after group move
                self.anchors_dirty = true;
                let frame = GroupFrame {
                    key,
                    start: cursor,
                    end: cursor + actual_len,
                    force_children_recompose: reused_gap || parent_force,
                };
                self.group_stack.push(frame);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            } else {
                // If the stored length exceeds available slots, fall back to inserting a new group.
                self.shift_group_frames(found_index, 0);
            }
        }

        self.insert_new_group_at_cursor(key)
    }

    fn insert_new_group_at_cursor(&mut self, key: Key) -> usize {
        // make sure we have space at the tail for pulling gaps
        self.ensure_capacity();

        let cursor = self.cursor;
        self.ensure_gap_at_local(cursor);
        let parent_force = self
            .group_stack
            .last()
            .map(|frame| frame.force_children_recompose)
            .unwrap_or(false);

        if cursor < self.slots.len() {
            debug_assert!(matches!(self.slots[cursor], Slot::Gap { .. }));
            let group_anchor = self.allocate_anchor();
            self.slots[cursor] = Slot::Group {
                key,
                anchor: group_anchor,
                len: 0,
                scope: None,
                has_gap_children: false,
            };
            self.register_anchor(group_anchor, cursor);
        } else {
            let group_anchor = self.allocate_anchor();
            self.slots.push(Slot::Group {
                key,
                anchor: group_anchor,
                len: 0,
                scope: None,
                has_gap_children: false,
            });
            self.register_anchor(group_anchor, cursor);
        }
        self.last_start_was_gap = parent_force;
        self.cursor = cursor + 1;
        self.group_stack.push(GroupFrame {
            key,
            start: cursor,
            end: self.cursor,
            force_children_recompose: parent_force,
        });
        self.update_group_bounds();
        cursor
    }
    #[allow(dead_code)]
    fn update_anchor_for_slot(&mut self, slot_index: usize) {
        let anchor_id = self.slots[slot_index].anchor_id().0;
        if anchor_id == 0 {
            return;
        }
        if anchor_id >= self.anchors.len() {
            self.anchors.resize(anchor_id + 1, INVALID_ANCHOR_POS);
        }
        self.anchors[anchor_id] = slot_index;
    }
    fn shift_anchor_positions_from(&mut self, start_slot: usize, delta: isize) {
        for pos in &mut self.anchors {
            if *pos != INVALID_ANCHOR_POS && *pos >= start_slot {
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
                    let old_len = *len;
                    if new_len < old_len {
                        *has_gap_children = true;
                    }
                    const SHRINK_MIN_DROP: usize = 64;
                    const SHRINK_RATIO: usize = 4;
                    if old_len > new_len
                        && old_len >= new_len.saturating_mul(SHRINK_RATIO)
                        && (old_len - new_len) >= SHRINK_MIN_DROP
                    {
                        *len = new_len;
                    } else {
                        *len = old_len.max(new_len);
                    }
                }
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
            if let Slot::Group { key, len, .. } = *slot {
                let frame = GroupFrame {
                    key,
                    start: index,
                    end: index + len,
                    force_children_recompose: false,
                };
                self.group_stack.push(frame);
                self.cursor = index + 1;
                if self.cursor < self.slots.len()
                    && matches!(self.slots.get(self.cursor), Some(Slot::Value { .. }))
                {
                    self.cursor += 1;
                }
            }
        }
    }

    fn end_recompose(&mut self) {
        if let Some(frame) = self.group_stack.pop() {
            self.cursor = frame.end;
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

    pub fn use_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> usize {
        self.ensure_capacity();

        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );

        if cursor < self.slots.len() {
            // Check if we can reuse the existing slot
            let reuse = matches!(
                self.slots.get(cursor),
                Some(Slot::Value { data, .. }) if data.is::<T>()
            );
            if reuse {
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }

            // Check if the slot is a Gap that we can replace
            if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                let anchor = self.allocate_anchor();
                let boxed: Box<dyn Any> = Box::new(init());
                self.slots[cursor] = Slot::Value {
                    anchor,
                    data: boxed,
                };
                self.register_anchor(anchor, cursor);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return cursor;
            }

            // Type mismatch: replace current slot (mark old content as unreachable via gap)
            // We replace in-place to maintain cursor position
            let anchor = self.allocate_anchor();
            let boxed: Box<dyn Any> = Box::new(init());
            self.slots[cursor] = Slot::Value {
                anchor,
                data: boxed,
            };
            self.register_anchor(anchor, cursor);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return cursor;
        }

        // We're at the end of the slot table, append new slot
        let anchor = self.allocate_anchor();
        let boxed: Box<dyn Any> = Box::new(init());
        let slot = Slot::Value {
            anchor,
            data: boxed,
        };
        self.slots.push(slot);
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
        *slot = Slot::Value {
            anchor,
            data: Box::new(value),
        };
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

    pub fn record_node(&mut self, id: NodeId) {
        self.ensure_capacity();

        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        if cursor < self.slots.len() {
            // Check if we can reuse the existing node slot
            if let Some(Slot::Node { id: existing, .. }) = self.slots.get(cursor) {
                if *existing == id {
                    self.cursor = cursor + 1;
                    self.update_group_bounds();
                    return;
                }
            }

            // Check if the slot is a Gap that we can replace
            if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                let anchor = self.allocate_anchor();
                self.slots[cursor] = Slot::Node { anchor, id };
                self.register_anchor(anchor, cursor);
                self.cursor = cursor + 1;
                self.update_group_bounds();
                return;
            }

            // Type mismatch: Replace the slot directly with the new node
            // For nodes, we can't use gaps because nodes exist in the applier
            // The old node becomes orphaned and will be garbage collected later
            let anchor = self.allocate_anchor();
            self.slots[cursor] = Slot::Node { anchor, id };
            self.register_anchor(anchor, cursor);
            self.cursor = cursor + 1;
            self.update_group_bounds();
            return;
        }

        // No existing slot at cursor: add new slot
        let anchor = self.allocate_anchor();
        let slot = Slot::Node { anchor, id };
        self.slots.push(slot);
        self.register_anchor(anchor, cursor);
        self.cursor = cursor + 1;
        self.update_group_bounds();
    }

    pub fn peek_node(&self) -> Option<NodeId> {
        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );
        match self.slots.get(cursor) {
            Some(Slot::Node { id, .. }) => Some(*id),
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
            if self.cursor < group_end {
                if self.mark_range_as_gaps(self.cursor, group_end, Some(owner_start)) {
                    marked = true;
                }
            }

            // Update the frame end to current cursor
            // NOTE: We do NOT update the group's len field, because gap slots
            // are still part of the group's physical extent in the slots array.
            // The group len should remain unchanged so that traversal can find the gaps.
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
        marked
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

    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group> {
        SlotTable::start_recompose_at_scope(self, scope).map(GroupId)
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

    fn peek_node(&self) -> Option<NodeId> {
        SlotTable::peek_node(self)
    }

    fn record_node(&mut self, id: NodeId) {
        SlotTable::record_node(self, id);
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

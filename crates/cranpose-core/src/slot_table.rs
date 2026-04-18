//! Slot table implementation using a gap-buffer strategy.
//!
//! This is the baseline/reference slot storage implementation that provides:
//! - Gap-based slot reuse during conditional rendering
//! - Anchor-based positional stability during reorganization
//! - Efficient group skipping and scope-based recomposition
//! - Batch anchor rebuilding for large structural changes

// Complex slot state machine logic benefits from explicit nested pattern matching for clarity
#![allow(clippy::collapsible_match)]

mod anchor_map;
mod lifecycle_queue;
mod reuse_planner;
#[cfg(test)]
mod tests;

use crate::{
    collections::map::HashMap,
    slot_storage::{GroupId, StartGroup, StartScopedGroup},
    AnchorId, Key, NodeId, Owned, RecomposeScope, ScopeId,
};
use anchor_map::AnchorMap;
pub use lifecycle_queue::OrphanedNode;
use lifecycle_queue::{OrphanedNodeIds, PendingSlotDrops};
use reuse_planner::{ReusePlanner, StartPlan};
use std::any::Any;

fn pack_slot_len(len: usize) -> u32 {
    u32::try_from(len).expect("slot length overflow")
}

fn unpack_slot_len(len: u32) -> usize {
    len as usize
}

#[derive(Clone, PartialEq, Eq)]
struct PreservedGroup {
    key: Key,
    len: u32,
    boundary_key: Key,
    scope: Option<RecomposeScope>,
}

impl std::fmt::Debug for PreservedGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreservedGroup")
            .field("key", &self.key)
            .field("len", &self.len)
            .field("boundary_key", &self.boundary_key)
            .field("scope_id", &self.scope.as_ref().map(RecomposeScope::id))
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
struct GapMetadata {
    extent: u32,
    preserved_group: Option<PreservedGroup>,
    preserved_node: Option<(NodeId, u32)>,
}

impl std::fmt::Debug for GapMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GapMetadata")
            .field("extent", &self.extent)
            .field("preserved_group", &self.preserved_group)
            .field("preserved_node", &self.preserved_node)
            .finish()
    }
}

impl Default for GapMetadata {
    fn default() -> Self {
        Self {
            extent: 1,
            preserved_group: None,
            preserved_node: None,
        }
    }
}

pub struct SlotTable {
    slots: Vec<Slot>,
    pending_slot_drops: PendingSlotDrops,
    cursor: usize,
    group_stack: Vec<GroupFrame>,
    anchor_map: AnchorMap,
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
    pub gap_metadata_len: usize,
    pub gap_metadata_cap: usize,
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

enum Slot {
    Group {
        key: Key,
        anchor: AnchorId,
        len: u32,
        boundary_key: Key,
        has_gap_children: bool,
        scope: Option<RecomposeScope>,
    },
    Value {
        anchor: AnchorId,
        data: Box<dyn SlotValue>,
    },
    Node {
        anchor: AnchorId,
        id: NodeId,
        gen: u32,
    },
    /// Gap: Marks an unused slot that can be reused or compacted.
    /// This prevents destructive truncation that would destroy sibling components.
    /// For Groups marked as gaps (e.g., during tab switching), we preserve their
    /// key, boundary, and length so they can be properly matched and reused when reactivated.
    Gap {
        anchor: AnchorId,
        metadata: GapMetadata,
    },
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

#[derive(Clone)]
struct MatchedGroup {
    index: usize,
    anchor: AnchorId,
    group: PreservedGroup,
    gap_boundary_key: Key,
    reused_gap: bool,
}

#[derive(Clone, Copy)]
struct GroupEntryPlan {
    child_reuse: ChildReusePolicy,
    fresh_body: bool,
    gap_boundary_key: Key,
    restored_from_gap: bool,
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

fn inherited_fresh_body(parent_reuse: ChildReusePolicy) -> bool {
    !matches!(parent_reuse, ChildReusePolicy::Normal)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeSlotState {
    Active,
    PreservedGap,
    Missing,
}

#[derive(Debug, PartialEq)]
enum SlotKind {
    Group,
    Value,
    Node,
    Gap,
}

impl Slot {
    fn placeholder_gap() -> Self {
        Slot::Gap {
            anchor: AnchorId::INVALID,
            metadata: GapMetadata::default(),
        }
    }

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

    fn gap_metadata(&self) -> Option<&GapMetadata> {
        match self {
            Slot::Gap { metadata, .. } => Some(metadata),
            _ => None,
        }
    }

    fn gap_extent(&self) -> usize {
        self.gap_metadata()
            .map(|metadata| unpack_slot_len(metadata.extent).max(1))
            .unwrap_or(1)
    }

    fn group_scope(&self) -> Option<&RecomposeScope> {
        match self {
            Slot::Group { scope, .. } => scope.as_ref(),
            _ => None,
        }
    }

    fn as_value<T: 'static>(&self) -> &T {
        match self {
            Slot::Value { data, .. } => data
                .as_any()
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
            _ => panic!("slot is not a value"),
        }
    }

    fn deactivate_scope(&self) {
        if let Slot::Group {
            scope: Some(scope), ..
        } = self
        {
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
            boundary_key: 0,
            has_gap_children: false,
            scope: None,
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
        Slot::Value {
            anchor,
            data: Box::new(TypedSlotValue(value)),
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
            anchor_map: AnchorMap::default(),
            last_start_was_gap: false,
            orphaned_node_ids: OrphanedNodeIds::default(),
            needs_compact: false,
        }
    }

    fn replace_slot_tracked(&mut self, index: usize, slot: Slot) -> Slot {
        std::mem::replace(&mut self.slots[index], slot)
    }

    fn set_slot_tracked(&mut self, index: usize, slot: Slot) {
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
        let group_stack_bytes = self.group_stack.capacity() * std::mem::size_of::<GroupFrame>();
        let orphaned_node_ids_bytes =
            self.orphaned_node_ids.capacity() * std::mem::size_of::<OrphanedNode>();
        slots_bytes
            + pending_bytes
            + self.anchor_map.debug_heap_bytes()
            + group_stack_bytes
            + orphaned_node_ids_bytes
    }

    pub fn debug_stats(&self) -> SlotTableDebugStats {
        let mut stats = SlotTableDebugStats {
            slots_len: self.slots.len(),
            slots_cap: self.slots.capacity(),
            pending_slot_drops_len: self.pending_slot_drops.len(),
            pending_slot_drops_cap: self.pending_slot_drops.capacity(),
            anchors_len: 0,
            anchors_cap: 0,
            gap_metadata_len: 0,
            gap_metadata_cap: 0,
            free_anchor_ids_len: 0,
            free_anchor_ids_cap: 0,
            group_stack_len: self.group_stack.len(),
            group_stack_cap: self.group_stack.capacity(),
            orphaned_node_ids_len: self.orphaned_node_ids.len(),
            orphaned_node_ids_cap: self.orphaned_node_ids.capacity(),
        };
        self.anchor_map.fill_debug_stats(&mut stats);
        stats
    }

    #[cfg(test)]
    fn gap_metadata_at_index(&self, index: usize) -> Option<GapMetadata> {
        self.slots.get(index).and_then(Slot::gap_metadata).cloned()
    }

    fn gap_extent_at(&self, index: usize) -> usize {
        self.slots.get(index).map(Slot::gap_extent).unwrap_or(1)
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

    fn group_scope_value(&self, group_index: usize) -> Option<&RecomposeScope> {
        self.slots.get(group_index).and_then(Slot::group_scope)
    }

    fn group_scope_owner(&self, group_index: usize) -> Option<ScopeId> {
        self.group_scope_value(group_index).map(RecomposeScope::id)
    }

    fn group_has_scope(&self, group_index: usize) -> bool {
        self.group_scope_value(group_index).is_some()
    }

    fn clear_group_scope(&mut self, group_index: usize) {
        if let Some(Slot::Group { scope, .. }) = self.slots.get_mut(group_index) {
            if let Some(existing_scope) = scope.take() {
                existing_scope.deactivate();
            }
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
        self.replace_slot_deferred(cursor, Slot::placeholder_gap());
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

    fn pull_gap_run_to_cursor(&mut self, cursor: usize, run_start: usize, moved_len: usize) {
        self.shift_group_frames(cursor, moved_len as isize);
        self.shift_anchor_positions_from(cursor, moved_len as isize);
        self.slots[cursor..run_start + moved_len].rotate_right(moved_len);
    }

    fn try_pull_gap_run_to_cursor(
        &mut self,
        cursor: usize,
        scan_limit: usize,
        move_entire_run: bool,
    ) -> bool {
        let Some((run_start, run_len)) = self.find_right_gap_run(cursor, scan_limit) else {
            return false;
        };
        let moved_len = if move_entire_run { run_len } else { 1 };
        self.pull_gap_run_to_cursor(cursor, run_start, moved_len);
        true
    }

    fn ensure_gap_at_local_with_mode(&mut self, cursor: usize, move_entire_run: bool) {
        if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
            return;
        }
        self.ensure_capacity();

        // Fast path: look for a gap run within the local scan window.
        if self.try_pull_gap_run_to_cursor(cursor, Self::LOCAL_GAP_SCAN, move_entire_run) {
            return;
        }

        // Slow path: after compaction the nearest gap may be far away.
        // Search the entire tail before falling back to the destructive force_gap_here.
        let full_range = self.slots.len().saturating_sub(cursor);
        if full_range > Self::LOCAL_GAP_SCAN
            && self.try_pull_gap_run_to_cursor(cursor, full_range, move_entire_run)
        {
            return;
        }

        // No gaps anywhere — grow the vec and try once more.
        self.grow_slots();
        let full_range = self.slots.len().saturating_sub(cursor);
        if self.try_pull_gap_run_to_cursor(cursor, full_range, move_entire_run) {
            return;
        }

        self.force_gap_here(cursor);
    }

    fn ensure_gap_at_local(&mut self, cursor: usize) {
        self.ensure_gap_at_local_with_mode(cursor, true);
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
            moved.push(self.replace_slot_tracked(index, Slot::placeholder_gap()));
        }
        self.anchor_map.mark_dirty();

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
            self.slots.push(Slot::placeholder_gap());
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
        self.anchor_map.allocate_anchor()
    }

    fn free_anchor(&mut self, anchor: AnchorId) {
        self.anchor_map.free_anchor(anchor);
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

    fn replace_gap_slot_deferred(&mut self, index: usize, preserved_group: Option<PreservedGroup>) {
        let anchor = self.slots[index].anchor_id();
        let (extent, preserved_node) = match self.slots.get(index) {
            Some(Slot::Node { id, gen, .. }) => (pack_slot_len(1), Some((*id, *gen))),
            Some(Slot::Gap { metadata, .. }) => {
                return self.replace_gap_slot_deferred_from_gap(
                    index,
                    anchor,
                    metadata.clone(),
                    preserved_group,
                );
            }
            _ => (
                preserved_group
                    .as_ref()
                    .map(|group| group.len)
                    .unwrap_or_else(|| pack_slot_len(1)),
                None,
            ),
        };
        let old = self.replace_slot_tracked(
            index,
            Slot::Gap {
                anchor,
                metadata: GapMetadata {
                    extent,
                    preserved_group,
                    preserved_node,
                },
            },
        );
        old.deactivate_scope();
        if let Slot::Node { id, gen, .. } = old {
            self.orphaned_node_ids
                .push(OrphanedNode::new(id, gen, anchor));
        }
        self.pending_slot_drops.push(old);
    }

    fn replace_gap_slot_deferred_from_gap(
        &mut self,
        index: usize,
        anchor: AnchorId,
        metadata: GapMetadata,
        preserved_group: Option<PreservedGroup>,
    ) {
        let old = self.replace_slot_tracked(
            index,
            Slot::Gap {
                anchor,
                metadata: GapMetadata {
                    extent: metadata.extent,
                    preserved_group: preserved_group.or(metadata.preserved_group.clone()),
                    preserved_node: metadata.preserved_node,
                },
            },
        );
        old.deactivate_scope();
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
        self.anchor_map.register_anchor(anchor, position);
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
        self.anchor_map.resolve_anchor(anchor)
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
        let mut marked_any = false;

        if !preserve_group_metadata {
            for index in start..end {
                self.replace_gap_slot_deferred(index, None);
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

            let (group_len, preserved_group) = {
                let slot = &self.slots[i];
                match slot {
                    Slot::Group {
                        len,
                        key,
                        boundary_key,
                        scope,
                        ..
                    } if preserve_group_metadata => (
                        *len,
                        Some(PreservedGroup {
                            key: *key,
                            len: *len,
                            boundary_key: *boundary_key,
                            scope: scope.clone(),
                        }),
                    ),
                    Slot::Group { len, .. } => (*len, None),
                    Slot::Gap { metadata, .. } => {
                        let preserved_group = preserve_group_metadata
                            .then_some(metadata.preserved_group.clone())
                            .flatten();
                        (
                            preserved_group.as_ref().map(|group| group.len).unwrap_or(0),
                            preserved_group,
                        )
                    }
                    _ => (0, None),
                }
            };

            self.replace_gap_slot_deferred(i, preserved_group);
            marked_any = true;

            // If it was a group, recursively mark its children as gaps too
            if group_len > 0 {
                // Mark children (from i+1 to i+group_len)
                let children_end = (i + unpack_slot_len(group_len)).min(end);
                for j in (i + 1)..children_end {
                    if j < self.slots.len() {
                        match &self.slots[j] {
                            Slot::Group {
                                key,
                                len,
                                boundary_key,
                                scope,
                                ..
                            } if preserve_group_metadata => {
                                self.replace_gap_slot_deferred(
                                    j,
                                    Some(PreservedGroup {
                                        key: *key,
                                        len: *len,
                                        boundary_key: *boundary_key,
                                        scope: scope.clone(),
                                    }),
                                );
                                marked_any = true;
                            }
                            Slot::Group { .. } => {
                                self.replace_gap_slot_deferred(j, None);
                                marked_any = true;
                            }
                            Slot::Gap { metadata, .. } if preserve_group_metadata => {
                                self.replace_gap_slot_deferred(j, metadata.preserved_group.clone());
                                marked_any = true;
                            }
                            Slot::Gap { .. } => {
                                self.replace_gap_slot_deferred(j, None);
                                marked_any = true;
                            }
                            Slot::Node { .. } => {
                                self.replace_gap_slot_deferred(j, None);
                                marked_any = true;
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

    #[cfg(test)]
    pub(crate) fn group_scope(&self, group: GroupId) -> Option<ScopeId> {
        self.group_scope_owner(group.0)
    }

    pub(crate) fn start_recranpose_at_anchor(
        &mut self,
        anchor: AnchorId,
        owner: ScopeId,
    ) -> Option<GroupId> {
        let index = self.resolve_anchor(anchor)?;
        if self.group_scope_owner(index) == Some(owner) {
            self.start_recompose(index);
            Some(GroupId(index))
        } else {
            None
        }
    }

    #[cfg(test)]
    pub fn find_group_index_by_scope(&self, scope: ScopeId) -> Option<GroupId> {
        self.slots
            .iter()
            .enumerate()
            .find_map(|(i, _)| (self.group_scope_owner(i) == Some(scope)).then_some(GroupId(i)))
    }

    #[cfg(test)]
    pub fn start_recranpose_at_scope(&mut self, scope: ScopeId) -> Option<GroupId> {
        let group = self.find_group_index_by_scope(scope)?;
        self.start_recompose(group.0);
        Some(group)
    }

    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                Slot::Group { key, len, .. } => {
                    Some((i, *key, self.group_scope_owner(i), unpack_slot_len(*len)))
                }
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
                    Slot::Group { key, len, .. } => format!(
                        "Group(key={:?}, scope={:?}, has_scope={}, len={})",
                        key,
                        self.group_scope_owner(i),
                        self.group_has_scope(i),
                        unpack_slot_len(*len)
                    ),
                    Slot::Value { .. } => "Value".to_string(),
                    Slot::Node { id, .. } => format!("Node(id={:?})", id),
                    Slot::Gap { metadata, .. } => {
                        if let Some(group) = &metadata.preserved_group {
                            format!(
                                "Gap(was_group_key={:?}, scope={:?})",
                                group.key,
                                group.scope.as_ref().map(RecomposeScope::id)
                            )
                        } else if let Some((id, gen)) = metadata.preserved_node {
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
                Slot::Group { scope: Some(_), .. } => Some((
                    std::any::type_name::<RecomposeScope>(),
                    std::mem::size_of::<RecomposeScope>(),
                )),
                Slot::Value { data, .. } => {
                    Some((data.debug_type_name(), data.debug_inline_payload_bytes()))
                }
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
        self.anchor_map.rebuild_all_positions(&self.slots);
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

    fn current_parent_reuse(&self) -> ChildReusePolicy {
        self.group_stack
            .last()
            .map(|frame| frame.child_reuse)
            .unwrap_or(ChildReusePolicy::Normal)
    }

    fn current_parent_end(&self) -> usize {
        self.group_stack
            .last()
            .map(|frame| frame.end.min(self.slots.len()))
            .unwrap_or(self.slots.len())
    }

    fn finish_slot_write_at(&mut self, cursor: usize) -> usize {
        self.cursor = cursor + 1;
        self.update_group_bounds();
        cursor
    }

    fn write_slot_at_cursor(&mut self, cursor: usize, slot: Slot) -> usize {
        let anchor = slot.anchor_id();
        self.set_slot_tracked(cursor, slot);
        if anchor.is_valid() {
            self.register_anchor(anchor, cursor);
        }
        self.finish_slot_write_at(cursor)
    }

    fn replace_gap_at_cursor_with_fresh_slot(
        &mut self,
        cursor: usize,
        make_slot: impl FnOnce(AnchorId) -> Slot,
    ) -> usize {
        let old_anchor = match self.slots.get(cursor) {
            Some(Slot::Gap { anchor, .. }) => *anchor,
            _ => panic!("expected gap at slot {}", cursor),
        };
        self.free_anchor(old_anchor);
        let anchor = self.allocate_anchor();
        self.write_slot_at_cursor(cursor, make_slot(anchor))
    }

    fn replace_slot_at_cursor_with_fresh_slot(
        &mut self,
        cursor: usize,
        make_slot: impl FnOnce(AnchorId) -> Slot,
    ) -> usize {
        let anchor = self.allocate_anchor();
        let slot = make_slot(anchor);
        self.replace_slot_deferred(cursor, slot);
        self.register_anchor(anchor, cursor);
        self.finish_slot_write_at(cursor)
    }

    fn append_fresh_slot_at_cursor(
        &mut self,
        cursor: usize,
        make_slot: impl FnOnce(AnchorId) -> Slot,
    ) -> usize {
        let anchor = self.allocate_anchor();
        let slot = make_slot(anchor);
        self.push_slot_tracked(slot);
        self.register_anchor(anchor, cursor);
        self.finish_slot_write_at(cursor)
    }

    fn materialize_slot_at_cursor(
        &mut self,
        disallow_live_reuse: bool,
        reusable_live_slot: impl Fn(&Slot) -> bool,
        mut preserved_gap_slot: impl FnMut(&mut Self, usize) -> Option<Slot>,
        make_fresh_slot: impl FnOnce(AnchorId) -> Slot,
    ) -> usize {
        self.ensure_capacity();

        let cursor = self.cursor;
        debug_assert!(
            cursor <= self.slots.len(),
            "slot cursor {} out of bounds",
            cursor
        );

        if cursor < self.slots.len() {
            let reusable_live_slot =
                !disallow_live_reuse && self.slots.get(cursor).is_some_and(reusable_live_slot);
            if disallow_live_reuse && !matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                self.ensure_gap_at_local(cursor);
            }

            if reusable_live_slot {
                return self.finish_slot_write_at(cursor);
            }

            if let Some(slot) = preserved_gap_slot(self, cursor) {
                return self.write_slot_at_cursor(cursor, slot);
            }

            if matches!(self.slots.get(cursor), Some(Slot::Gap { .. })) {
                return self.replace_gap_at_cursor_with_fresh_slot(cursor, make_fresh_slot);
            }

            return self.replace_slot_at_cursor_with_fresh_slot(cursor, make_fresh_slot);
        }

        self.append_fresh_slot_at_cursor(cursor, make_fresh_slot)
    }

    fn move_slot_range_to_cursor(&mut self, source_start: usize, len: usize, dest: usize) {
        debug_assert!(dest <= source_start, "unexpected rightward slot move");
        if len == 0 || source_start == dest {
            return;
        }

        self.shift_group_frames(source_start, -(len as isize));
        let moved: Vec<_> = self.slots.drain(source_start..source_start + len).collect();
        self.shift_group_frames(dest, moved.len() as isize);
        self.slots.splice(dest..dest, moved);
        self.anchor_map.mark_dirty();
    }

    fn enter_group(&mut self, key: Key, start: usize, len: usize, plan: GroupEntryPlan) -> usize {
        self.last_start_was_gap = plan.restored_from_gap;
        self.group_stack.push(GroupFrame {
            key,
            start,
            end: start + len,
            child_reuse: plan.child_reuse,
            fresh_body: plan.fresh_body,
            gap_boundary_key: plan.gap_boundary_key,
        });
        self.cursor = start + 1;
        self.update_group_bounds();
        start
    }

    fn retire_conflicting_group_at_cursor(&mut self, key: Key, cursor: usize) {
        let Some(Slot::Group {
            key: existing_key,
            len: old_len,
            boundary_key: old_boundary_key,
            ..
        }) = self.slots.get(cursor)
        else {
            return;
        };

        if *existing_key == key {
            return;
        }

        let old_key = *existing_key;
        let old_len = unpack_slot_len(*old_len);
        let old_boundary_key = *old_boundary_key;
        let parent_end = self.current_parent_end();
        let preserve_at_tail = cursor + old_len == parent_end;

        if old_len > 1 {
            let start = cursor + 1;
            let end = cursor + old_len;
            let _ = self.mark_range_as_gaps_impl(start, end, Some(cursor), false);
        }

        self.replace_gap_slot_deferred(
            cursor,
            Some(PreservedGroup {
                key: old_key,
                len: pack_slot_len(old_len),
                boundary_key: old_boundary_key,
                scope: self.group_scope_value(cursor).cloned(),
            }),
        );

        if preserve_at_tail {
            self.preserve_terminal_group_block_at_tail(cursor, old_len);
        }
    }

    fn restore_gap_group_at_cursor(
        &mut self,
        key: Key,
        cursor: usize,
        parent_reuse: ChildReusePolicy,
        anchor: AnchorId,
        group: PreservedGroup,
    ) -> Option<usize> {
        if group.key != key {
            return None;
        }

        let len = unpack_slot_len(group.len);
        let group_anchor = if anchor.is_valid() {
            anchor
        } else {
            self.allocate_anchor()
        };
        let gap_boundary_key = if matches!(parent_reuse, ChildReusePolicy::FreshInsert) {
            self.next_gap_boundary_key(key, parent_reuse)
        } else {
            group.boundary_key
        };
        let child_reuse = if matches!(parent_reuse, ChildReusePolicy::FreshInsert) {
            ChildReusePolicy::FreshInsert
        } else {
            ChildReusePolicy::ParentRestoredFromGap
        };

        self.write_slot_at_cursor(
            cursor,
            Slot::Group {
                key,
                anchor: group_anchor,
                len: pack_slot_len(len),
                boundary_key: gap_boundary_key,
                has_gap_children: false,
                scope: group.scope.clone(),
            },
        );
        Some(self.enter_group(
            key,
            cursor,
            len,
            GroupEntryPlan {
                child_reuse,
                fresh_body: true,
                gap_boundary_key,
                restored_from_gap: true,
            },
        ))
    }

    fn try_restore_matching_group(
        &mut self,
        key: Key,
        cursor: usize,
        parent_reuse: ChildReusePolicy,
        matched: MatchedGroup,
    ) -> Option<usize> {
        let restored_boundary_key =
            if matched.reused_gap && matches!(parent_reuse, ChildReusePolicy::FreshInsert) {
                self.next_gap_boundary_key(key, parent_reuse)
            } else {
                matched.gap_boundary_key
            };
        if matched.reused_gap {
            self.set_slot_tracked(
                matched.index,
                Slot::Group {
                    key,
                    anchor: matched.anchor,
                    len: matched.group.len,
                    boundary_key: restored_boundary_key,
                    has_gap_children: false,
                    scope: matched.group.scope.clone(),
                },
            );
        }

        let restored_from_preserved = matched.reused_gap || matched.index != cursor;
        let inherited_fresh_body = inherited_fresh_body(parent_reuse);
        let actual_len = unpack_slot_len(matched.group.len)
            .max(1)
            .min(self.slots.len().saturating_sub(matched.index));
        if actual_len == 0 {
            return None;
        }

        self.move_slot_range_to_cursor(matched.index, actual_len, cursor);
        let child_reuse = if restored_from_preserved {
            restored_child_reuse(parent_reuse)
        } else {
            parent_reuse
        };
        Some(self.enter_group(
            key,
            cursor,
            actual_len,
            GroupEntryPlan {
                child_reuse,
                fresh_body: restored_from_preserved || inherited_fresh_body,
                gap_boundary_key: restored_boundary_key,
                restored_from_gap: restored_from_preserved || inherited_fresh_body,
            },
        ))
    }

    pub(crate) fn start(&mut self, key: Key) -> usize {
        self.ensure_capacity();

        let cursor = self.cursor;
        let parent_reuse = self.current_parent_reuse();
        self.last_start_was_gap = false;
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

        let plan = ReusePlanner::new(
            &self.slots,
            key,
            cursor,
            self.current_parent_end(),
            parent_reuse,
            self.current_parent_gap_boundary_key(),
        )
        .plan();

        match plan {
            StartPlan::ReuseLiveAtCursor {
                len,
                gap_boundary_key,
            } => {
                return self.enter_group(
                    key,
                    cursor,
                    len,
                    GroupEntryPlan {
                        child_reuse: parent_reuse,
                        fresh_body: inherited_fresh_body(parent_reuse),
                        gap_boundary_key,
                        restored_from_gap: inherited_fresh_body(parent_reuse),
                    },
                );
            }
            StartPlan::RestoreGapAtCursor { anchor, group } => {
                if let Some(restored) =
                    self.restore_gap_group_at_cursor(key, cursor, parent_reuse, anchor, group)
                {
                    return restored;
                }
            }
            StartPlan::RestoreMatchingGroup {
                matched_group,
                retire_conflicting_group_at_cursor,
            } => {
                if retire_conflicting_group_at_cursor {
                    self.retire_conflicting_group_at_cursor(key, cursor);
                }
                if let Some(restored) =
                    self.try_restore_matching_group(key, cursor, parent_reuse, matched_group)
                {
                    return restored;
                }
            }
            StartPlan::InsertFresh {
                retire_conflicting_group_at_cursor,
            } => {
                if retire_conflicting_group_at_cursor {
                    self.retire_conflicting_group_at_cursor(key, cursor);
                }
            }
        }

        self.insert_new_group_at_cursor(key, ChildReusePolicy::FreshInsert)
    }

    fn insert_new_group_at_cursor(&mut self, key: Key, child_reuse: ChildReusePolicy) -> usize {
        // make sure we have space at the tail for pulling gaps
        self.ensure_capacity();

        let cursor = self.cursor;
        self.ensure_gap_at_local(cursor);
        let gap_boundary_key = self.next_gap_boundary_key(key, child_reuse);

        if cursor < self.slots.len() {
            debug_assert!(matches!(self.slots[cursor], Slot::Gap { .. }));
            if let Some(Slot::Gap {
                anchor: old_anchor, ..
            }) = self.slots.get(cursor)
            {
                self.free_anchor(*old_anchor);
            }
            let group_anchor = self.allocate_anchor();
            self.set_slot_tracked(
                cursor,
                Slot::Group {
                    key,
                    anchor: group_anchor,
                    len: 0,
                    boundary_key: gap_boundary_key,
                    has_gap_children: false,
                    scope: None,
                },
            );
            self.register_anchor(group_anchor, cursor);
        } else {
            let group_anchor = self.allocate_anchor();
            self.push_slot_tracked(Slot::Group {
                key,
                anchor: group_anchor,
                len: 0,
                boundary_key: gap_boundary_key,
                has_gap_children: false,
                scope: None,
            });
            self.register_anchor(group_anchor, cursor);
        }
        self.enter_group(
            key,
            cursor,
            0,
            GroupEntryPlan {
                child_reuse,
                fresh_body: true,
                gap_boundary_key,
                restored_from_gap: false,
            },
        )
    }
    fn shift_anchor_positions_from(&mut self, start_slot: usize, delta: isize) {
        self.anchor_map.shift_positions_from(start_slot, delta);
    }
    fn flush_anchors_if_dirty(&mut self) {
        if self.anchor_map.take_dirty() {
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
            }
        }
    }

    pub fn end_recompose(&mut self) {
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
            if matches!(self.slots.get(i), Some(Slot::Gap { .. })) {
                i += self.gap_extent_at(i);
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
            let Slot::Group {
                scope: Some(scope), ..
            } = slot
            else {
                continue;
            };

            if scope.id() == current_scope || seen.insert(scope.id(), ()).is_some() {
                continue;
            }

            scopes.push(scope.clone());
        }

        scopes
    }

    fn preserved_gap_node_at_cursor(&self, cursor: usize) -> Option<(AnchorId, NodeId, u32)> {
        let Slot::Gap { anchor, metadata } = self.slots.get(cursor)? else {
            return None;
        };
        if !self.current_parent_allows_exact_gap_node_reuse() {
            return None;
        }
        metadata.preserved_node.map(|(id, gen)| (*anchor, id, gen))
    }

    pub fn use_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> usize {
        let disallow_live_reuse = self.current_disallow_live_slot_reuse();
        self.materialize_slot_at_cursor(
            disallow_live_reuse,
            |slot| matches!(slot, Slot::Value { data, .. } if data.as_any().is::<T>()),
            |_table, _cursor| None,
            |anchor| Self::make_value_slot(anchor, init()),
        )
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
        self.materialize_slot_at_cursor(
            false,
            |slot| {
                matches!(
                    slot,
                    Slot::Node {
                        id: existing,
                        gen: existing_gen,
                        ..
                    } if *existing == id && *existing_gen == gen
                )
            },
            |table, cursor| {
                let (old_anchor, preserved_id, preserved_gen) =
                    table.preserved_gap_node_at_cursor(cursor)?;
                if (preserved_id, preserved_gen) != (id, gen) {
                    return None;
                }
                let anchor = if old_anchor.is_valid() {
                    old_anchor
                } else {
                    table.allocate_anchor()
                };
                Some(Slot::Node { anchor, id, gen })
            },
            |anchor| Slot::Node { anchor, id, gen },
        );
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
            Some(Slot::Gap { .. }) => self
                .preserved_gap_node_at_cursor(cursor)
                .map(|(_, id, gen)| (id, gen)),
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
        let preserved = self.preserved_gap_node_at_cursor(cursor);
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
            Some(Slot::Gap { metadata, .. })
                if metadata.preserved_node == Some((orphaned.id, orphaned.generation)) =>
            {
                NodeSlotState::PreservedGap
            }
            _ => NodeSlotState::Missing,
        }
    }

    #[cfg(test)]
    fn gap_metadata_at(&self, index: usize) -> Option<GapMetadata> {
        self.gap_metadata_at_index(index)
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
                Slot::Gap { metadata, .. } => {
                    let extent =
                        unpack_slot_len(metadata.extent).min(old_len.saturating_sub(gap_scan));
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
                Slot::Gap { metadata, .. } => {
                    let extent = unpack_slot_len(metadata.extent).min(old_len.saturating_sub(i));
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
                Slot::Value { .. } | Slot::Node { .. } => {}
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
            if let Slot::Gap { metadata, .. } = &self.slots[i] {
                let extent = unpack_slot_len(metadata.extent).min(old_len.saturating_sub(i));
                for j in i..i + extent {
                    let anchor = self.slots[j].anchor_id();
                    if anchor.is_valid() {
                        self.anchor_map.remove_position(anchor);
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
            if let Slot::Gap { metadata, .. } = &self.slots[read] {
                let extent = unpack_slot_len(metadata.extent).min(old_len.saturating_sub(read));
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
        self.anchor_map.rebuild_positions(&self.slots);
    }
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SlotTable {
    pub fn begin_group(&mut self, key: Key) -> StartGroup<GroupId> {
        let idx = SlotTable::start(self, key);
        self.clear_group_scope(idx);
        let restored = SlotTable::take_last_start_was_gap(self);
        StartGroup {
            group: GroupId(idx),
            anchor: self.slots[idx].anchor_id(),
            restored_from_gap: restored,
        }
    }

    pub fn begin_scoped_group(
        &mut self,
        key: Key,
        init_scope: impl FnOnce() -> RecomposeScope,
    ) -> StartScopedGroup<GroupId> {
        let idx = SlotTable::start(self, key);
        let restored = SlotTable::take_last_start_was_gap(self);
        let scope = if let Some(existing_scope) = self.group_scope_value(idx).cloned() {
            existing_scope
        } else {
            let scope = init_scope();
            if let Some(Slot::Group {
                scope: stored_scope,
                ..
            }) = self.slots.get_mut(idx)
            {
                *stored_scope = Some(scope.clone());
            }
            scope
        };
        StartScopedGroup {
            group: GroupId(idx),
            anchor: self.slots[idx].anchor_id(),
            scope,
            restored_from_gap: restored,
        }
    }

    pub fn end_group(&mut self) {
        SlotTable::end(self);
    }

    pub fn skip_current_group(&mut self) {
        SlotTable::skip_current(self);
    }

    pub fn nodes_in_current_group(&self) -> Vec<NodeId> {
        SlotTable::node_ids_in_current_group(self)
    }

    pub fn finalize_current_group(&mut self) -> bool {
        SlotTable::trim_to_cursor(self)
    }

    pub fn flush(&mut self) {
        SlotTable::flush_anchors_if_dirty(self);
    }
}

impl Drop for SlotTable {
    fn drop(&mut self) {
        self.flush_pending_slot_drops();
        drop_slots_in_reverse(&mut self.slots);
    }
}

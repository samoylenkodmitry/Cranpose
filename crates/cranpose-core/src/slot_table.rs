//! Slot table implementation with a single logical gap buffer and explicit hidden entries.

mod anchor_map;
mod lifecycle_queue;
mod storage;

use crate::{
    remove_child_and_cleanup_now,
    slot_storage::{GroupId, StartScopedGroup},
    AnchorId, Applier, Key, NodeId, Owned, RecomposeScope, ScopeId,
};
use lifecycle_queue::DeferredDrop;
pub use lifecycle_queue::OrphanedNode;
pub(crate) use lifecycle_queue::SlotLifecycleCoordinator;
use storage::{EntryKind, GroupSnapshot, SlotStorage, StorageLifecycleEvent};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeSlotState {
    Active,
    PreservedGap,
    Missing,
}

fn unpack_group_extent(extent: u32) -> usize {
    extent as usize
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PassBoundary {
    Open,
    Restored { boundary_key: Key },
    Fresh { boundary_key: Key },
}

impl PassBoundary {
    fn restricted_boundary(self) -> Option<Key> {
        match self {
            Self::Open => None,
            Self::Restored { boundary_key } | Self::Fresh { boundary_key } => Some(boundary_key),
        }
    }

    fn inherited_boundary(self, key: Key) -> Key {
        self.restricted_boundary().unwrap_or(key)
    }

    fn allows_exact_live_reuse(self) -> bool {
        !matches!(self, Self::Fresh { .. })
    }

    fn allows_live_search(self) -> bool {
        matches!(self, Self::Open)
    }

    fn disallows_live_value_reuse(self) -> bool {
        matches!(self, Self::Fresh { .. })
    }

    fn child_for_restored(self, boundary_key: Key) -> Self {
        if matches!(self, Self::Fresh { .. }) {
            Self::Fresh { boundary_key }
        } else {
            Self::Restored { boundary_key }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotPassMode {
    Compose,
    Recompose,
}

#[derive(Clone, Debug)]
struct GroupFrame {
    start: usize,
    end: usize,
    pass_boundary: PassBoundary,
}

#[derive(Clone)]
struct MatchedGroup {
    index: usize,
    group: GroupSnapshot,
    reused_hidden: bool,
}

#[derive(Clone, Copy, Debug)]
struct GroupEntryPlan {
    pass_boundary: PassBoundary,
    restored_from_hidden: bool,
}

#[derive(Clone, Copy, Debug)]
struct StartedGroup {
    index: usize,
    restored_from_hidden: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GroupRetention {
    Clean { boundary_key: Key },
    Preserved { boundary_key: Key },
}

impl GroupRetention {
    pub(crate) fn clean(boundary_key: Key) -> Self {
        Self::Clean { boundary_key }
    }

    fn preserved(boundary_key: Key) -> Self {
        Self::Preserved { boundary_key }
    }

    pub(crate) fn boundary_key(self) -> Key {
        match self {
            Self::Clean { boundary_key } | Self::Preserved { boundary_key } => boundary_key,
        }
    }

    fn is_preserved(self) -> bool {
        matches!(self, Self::Preserved { .. })
    }

    fn preserve(self) -> Self {
        Self::Preserved {
            boundary_key: self.boundary_key(),
        }
    }
}

#[derive(Default)]
pub(crate) struct SlotWriteSessionState {
    cursor: usize,
    group_stack: Vec<GroupFrame>,
}

pub(crate) struct SlotWriteSession<'a> {
    table: &'a mut SlotTable,
    lifecycle: &'a mut SlotLifecycleCoordinator,
    state: &'a mut SlotWriteSessionState,
    mode: SlotPassMode,
}

struct SlotReadCursor<'a> {
    table: &'a SlotTable,
}

mod reuse_planner;
#[cfg(test)]
mod tests;

use reuse_planner::{ReusePlanner, StartPlan};

pub struct SlotTable {
    storage: SlotStorage,
}

impl SlotTable {
    const INITIAL_CAP: usize = 32;
    const EAGER_COMPACT_SLOT_LEN: usize = 1_024;
    const FRACTIONAL_COMPACT_GAP_THRESHOLD: usize = 256;
    const FRACTIONAL_COMPACT_RATIO_DIVISOR: usize = 4;
    const LARGE_GROWTH_THRESHOLD: usize = 32 * 1024;
    const LARGE_GROWTH_DIVISOR: usize = 4;

    pub fn new() -> Self {
        Self {
            storage: SlotStorage::new(),
        }
    }

    pub fn heap_bytes(&self) -> usize {
        self.storage.heap_bytes()
    }

    pub fn debug_stats(&self) -> SlotTableDebugStats {
        self.storage.debug_stats()
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

    fn ensure_insert_capacity(&mut self, index: usize) {
        if self.storage.gap_len() == 0 {
            let current = self.storage.capacity();
            let target = Self::next_slot_target_len(current.max(self.storage.len()));
            self.storage
                .grow(target.max(self.storage.len().saturating_add(1)));
        }
        self.storage.ensure_gap_at(index);
    }

    pub(crate) fn write_session<'a>(
        &'a mut self,
        lifecycle: &'a mut SlotLifecycleCoordinator,
        state: &'a mut SlotWriteSessionState,
        mode: SlotPassMode,
    ) -> SlotWriteSession<'a> {
        SlotWriteSession {
            table: self,
            lifecycle,
            state,
            mode,
        }
    }

    fn update_group_bounds(&self, state: &mut SlotWriteSessionState) {
        for frame in &mut state.group_stack {
            if frame.end < state.cursor {
                frame.end = state.cursor;
            }
        }
    }

    fn shift_group_frames(&self, state: &mut SlotWriteSessionState, index: usize, delta: isize) {
        if delta == 0 {
            return;
        }

        if delta > 0 {
            let delta = delta as usize;
            for frame in &mut state.group_stack {
                if frame.start >= index {
                    frame.start += delta;
                    frame.end += delta;
                } else if frame.end >= index {
                    frame.end += delta;
                }
            }
        } else {
            let delta = (-delta) as usize;
            for frame in &mut state.group_stack {
                if frame.start >= index {
                    frame.start = frame.start.saturating_sub(delta);
                    frame.end = frame.end.saturating_sub(delta);
                } else if frame.end > index {
                    frame.end = frame.end.saturating_sub(delta);
                }
            }
        }
    }

    fn finish_slot_write_at(&self, state: &mut SlotWriteSessionState, index: usize) -> usize {
        state.cursor = index + 1;
        self.update_group_bounds(state);
        index
    }

    fn current_parent_boundary(&self, state: &SlotWriteSessionState) -> PassBoundary {
        state
            .group_stack
            .last()
            .map(|frame| frame.pass_boundary)
            .unwrap_or(PassBoundary::Open)
    }

    fn current_parent_end(&self, state: &SlotWriteSessionState) -> usize {
        state
            .group_stack
            .last()
            .map(|frame| frame.end.min(self.storage.len()))
            .unwrap_or(self.storage.len())
    }

    fn current_parent_anchor(&self, state: &SlotWriteSessionState) -> AnchorId {
        state
            .group_stack
            .last()
            .map(|frame| self.storage.entry_anchor(frame.start))
            .unwrap_or(AnchorId::INVALID)
    }

    fn current_parent_boundary_key(&self, state: &SlotWriteSessionState) -> Option<Key> {
        state
            .group_stack
            .last()
            .and_then(|frame| frame.pass_boundary.restricted_boundary())
    }

    fn current_disallow_live_slot_reuse(&self, state: &SlotWriteSessionState) -> bool {
        state
            .group_stack
            .last()
            .is_some_and(|frame| frame.pass_boundary.disallows_live_value_reuse())
    }

    fn current_parent_allows_exact_hidden_node_reuse(&self, state: &SlotWriteSessionState) -> bool {
        state
            .group_stack
            .last()
            .map(|frame| frame.pass_boundary.allows_exact_live_reuse())
            .unwrap_or(true)
    }

    fn group_scope_value(&self, group_index: usize) -> Option<&RecomposeScope> {
        self.storage.live_group_scope(group_index)
    }

    fn group_scope_owner(&self, group_index: usize) -> Option<ScopeId> {
        self.group_scope_value(group_index).map(RecomposeScope::id)
    }

    fn group_has_scope(&self, group_index: usize) -> bool {
        self.storage.live_group_has_scope(group_index)
    }

    #[cfg(test)]
    fn clear_group_scope(&mut self, group_index: usize) {
        self.storage.clear_group_scope(group_index);
    }

    fn move_slot_range_to_cursor(
        &mut self,
        state: &mut SlotWriteSessionState,
        source_start: usize,
        len: usize,
        dest: usize,
    ) {
        debug_assert!(dest <= source_start, "unexpected rightward slot move");
        if len == 0 || source_start == dest {
            return;
        }

        let removed = self.storage.remove_entry_range(source_start, len);
        self.shift_group_frames(state, source_start, -(len as isize));
        self.ensure_insert_capacity(dest);
        self.shift_group_frames(state, dest, len as isize);
        self.storage.insert_entry_range(dest, &removed);
    }

    fn enter_group(
        &self,
        state: &mut SlotWriteSessionState,
        start: usize,
        len: usize,
        plan: GroupEntryPlan,
    ) -> StartedGroup {
        state.group_stack.push(GroupFrame {
            start,
            end: start + len,
            pass_boundary: plan.pass_boundary,
        });
        state.cursor = start + 1;
        self.update_group_bounds(state);
        StartedGroup {
            index: start,
            restored_from_hidden: plan.restored_from_hidden,
        }
    }

    fn insert_group_entry(
        &mut self,
        state: &mut SlotWriteSessionState,
        index: usize,
        key: Key,
        boundary_key: Key,
        scope: Option<RecomposeScope>,
    ) -> AnchorId {
        self.ensure_insert_capacity(index);
        let anchor = self.storage.allocate_anchor();
        let group = self.storage.alloc_group(
            key,
            GroupRetention::clean(boundary_key),
            self.current_parent_anchor(state),
            scope,
        );
        self.shift_group_frames(state, index, 1);
        self.storage.insert_group(index, anchor, group, false);
        anchor
    }

    fn insert_value_entry<T: 'static>(
        &mut self,
        state: &mut SlotWriteSessionState,
        index: usize,
        value: T,
    ) {
        self.ensure_insert_capacity(index);
        let anchor = self.storage.allocate_anchor();
        let payload = self.storage.alloc_value(value);
        self.shift_group_frames(state, index, 1);
        self.storage.insert_value(index, anchor, payload, false);
    }

    fn overwrite_value_entry<T: 'static>(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        index: usize,
        value: T,
        hidden: bool,
    ) {
        let anchor = self.storage.entry_anchor(index);
        let payload = self.storage.alloc_value(value);
        if let Some(deferred_drop) = self.storage.overwrite_value(index, anchor, payload, hidden) {
            lifecycle.push_drop(deferred_drop);
        }
    }

    fn insert_node_entry(
        &mut self,
        state: &mut SlotWriteSessionState,
        index: usize,
        id: NodeId,
        generation: u32,
    ) {
        self.ensure_insert_capacity(index);
        let anchor = self.storage.allocate_anchor();
        let payload = self.storage.alloc_node(id, generation);
        self.shift_group_frames(state, index, 1);
        self.storage.insert_node(index, anchor, payload, false);
    }

    fn overwrite_node_entry(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        index: usize,
        id: NodeId,
        generation: u32,
        hidden: bool,
    ) {
        let anchor = self.storage.entry_anchor(index);
        let payload = self.storage.alloc_node(id, generation);
        if let Some(deferred_drop) = self.storage.overwrite_node(index, anchor, payload, hidden) {
            lifecycle.push_drop(deferred_drop);
        }
    }

    fn mark_range_as_hidden(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        start: usize,
        end: usize,
        owner_index: Option<usize>,
    ) -> bool {
        let end = end.min(self.storage.len());
        let mut index = start;
        let mut marked_any = false;

        while index < end {
            let Some(kind) = self.storage.entry_kind(index) else {
                break;
            };
            let extent = self.storage.entry_extent(index).max(1);
            match kind {
                EntryKind::Group => {
                    let subtree_end = (index + extent).min(end);
                    for child in index..subtree_end {
                        let event = self.storage.hide_entry(child);
                        Self::handle_lifecycle_event(lifecycle, event);
                        marked_any = true;
                    }
                    if subtree_end > index + 1 {
                        self.preserve_group_retention(index);
                    }
                    index = subtree_end;
                }
                EntryKind::Value | EntryKind::Node => {
                    let event = self.storage.hide_entry(index);
                    Self::handle_lifecycle_event(lifecycle, event);
                    marked_any = true;
                    index += 1;
                }
                EntryKind::HiddenGroup | EntryKind::HiddenValue | EntryKind::HiddenNode => {
                    index += extent;
                }
                EntryKind::Unused => {
                    index += 1;
                }
            }
        }

        if marked_any {
            self.storage.needs_compact = true;
            if let Some(owner_index) = owner_index {
                self.preserve_group_retention(owner_index);
            }
        }

        marked_any
    }

    fn preserve_group_retention(&mut self, index: usize) {
        if let Some(retention) = self.storage.group_retention_at(index) {
            self.storage
                .set_group_retention(index, retention.preserve());
        }
    }

    fn set_group_boundary_key(&mut self, index: usize, boundary_key: Key) {
        if let Some(retention) = self.storage.group_retention_at(index) {
            let next_retention = if retention.is_preserved() {
                GroupRetention::preserved(boundary_key)
            } else {
                GroupRetention::clean(boundary_key)
            };
            self.storage.set_group_retention(index, next_retention);
        }
    }

    fn handle_lifecycle_event(
        lifecycle: &mut SlotLifecycleCoordinator,
        event: Option<StorageLifecycleEvent>,
    ) {
        match event {
            Some(StorageLifecycleEvent::DeactivateScope(scope)) => {
                lifecycle.deactivate_scope(&scope);
            }
            Some(StorageLifecycleEvent::OrphanNode(orphaned)) => {
                lifecycle.queue_orphaned_node(orphaned);
            }
            None => {}
        }
    }

    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        SlotReadCursor::new(self).collect_group_debug_rows()
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        SlotReadCursor::new(self).collect_slot_debug_rows()
    }

    pub fn debug_value_type_counts(&self, limit: usize) -> Vec<SlotValueTypeDebugStat> {
        self.storage.debug_value_type_counts(limit)
    }

    fn retire_conflicting_group_at_cursor(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        state: &SlotWriteSessionState,
        key: Key,
        cursor: usize,
    ) {
        let Some(group) = self.storage.group_snapshot_at(cursor) else {
            return;
        };
        if self.storage.entry_kind(cursor) != Some(EntryKind::Group) || group.key == key {
            return;
        }

        let old_extent = unpack_group_extent(group.extent).max(1);
        let owner_index = state.group_stack.last().map(|frame| frame.start);
        let _ = self.mark_range_as_hidden(lifecycle, cursor, cursor + old_extent, owner_index);
    }

    fn restore_hidden_group_at_cursor(
        &mut self,
        state: &mut SlotWriteSessionState,
        key: Key,
        cursor: usize,
        parent_boundary: PassBoundary,
        group: GroupSnapshot,
    ) -> Option<StartedGroup> {
        if self.storage.entry_kind(cursor) != Some(EntryKind::HiddenGroup) || group.key != key {
            return None;
        }

        let boundary_key = if matches!(parent_boundary, PassBoundary::Fresh { .. }) {
            parent_boundary.inherited_boundary(key)
        } else {
            group.retention.boundary_key()
        };
        let pass_boundary = parent_boundary.child_for_restored(boundary_key);

        self.storage.restore_hidden_entry(cursor);
        self.set_group_boundary_key(cursor, boundary_key);
        Some(self.enter_group(
            state,
            cursor,
            unpack_group_extent(group.extent),
            GroupEntryPlan {
                pass_boundary,
                restored_from_hidden: true,
            },
        ))
    }

    fn try_restore_matching_group(
        &mut self,
        state: &mut SlotWriteSessionState,
        key: Key,
        cursor: usize,
        parent_boundary: PassBoundary,
        matched: MatchedGroup,
    ) -> Option<StartedGroup> {
        let restored_boundary_key =
            if matched.reused_hidden && matches!(parent_boundary, PassBoundary::Fresh { .. }) {
                parent_boundary.inherited_boundary(key)
            } else {
                matched.group.retention.boundary_key()
            };

        if matched.reused_hidden {
            self.storage.restore_hidden_entry(matched.index);
            self.set_group_boundary_key(matched.index, restored_boundary_key);
        }

        let restored_from_hidden = matched.reused_hidden || matched.index != cursor;
        let actual_extent = unpack_group_extent(matched.group.extent)
            .max(1)
            .min(self.storage.len().saturating_sub(matched.index));
        if actual_extent == 0 {
            return None;
        }

        self.move_slot_range_to_cursor(state, matched.index, actual_extent, cursor);
        let pass_boundary = if restored_from_hidden {
            parent_boundary.child_for_restored(restored_boundary_key)
        } else {
            match parent_boundary {
                PassBoundary::Open => PassBoundary::Open,
                PassBoundary::Restored { .. } => PassBoundary::Restored {
                    boundary_key: restored_boundary_key,
                },
                PassBoundary::Fresh { .. } => PassBoundary::Fresh {
                    boundary_key: restored_boundary_key,
                },
            }
        };

        Some(self.enter_group(
            state,
            cursor,
            actual_extent,
            GroupEntryPlan {
                pass_boundary,
                restored_from_hidden,
            },
        ))
    }

    fn insert_new_group_at_cursor(
        &mut self,
        state: &mut SlotWriteSessionState,
        key: Key,
        pass_boundary: PassBoundary,
    ) -> StartedGroup {
        let boundary_key = pass_boundary.inherited_boundary(key);
        self.insert_group_entry(state, state.cursor, key, boundary_key, None);
        self.enter_group(
            state,
            state.cursor,
            1,
            GroupEntryPlan {
                pass_boundary,
                restored_from_hidden: false,
            },
        )
    }

    fn start_group_entry(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        state: &mut SlotWriteSessionState,
        key: Key,
    ) -> StartedGroup {
        let cursor = state.cursor;
        let parent_boundary = self.current_parent_boundary(state);

        let plan = ReusePlanner::new(
            &self.storage,
            key,
            cursor,
            self.current_parent_end(state),
            parent_boundary,
            self.current_parent_boundary_key(state),
        )
        .plan();

        match plan {
            StartPlan::ReuseLiveAtCursor {
                extent,
                boundary_key,
            } => {
                let pass_boundary = match parent_boundary {
                    PassBoundary::Open => PassBoundary::Open,
                    PassBoundary::Restored { .. } => PassBoundary::Restored { boundary_key },
                    PassBoundary::Fresh { .. } => PassBoundary::Fresh { boundary_key },
                };
                return self.enter_group(
                    state,
                    cursor,
                    extent,
                    GroupEntryPlan {
                        pass_boundary,
                        restored_from_hidden: !matches!(pass_boundary, PassBoundary::Open),
                    },
                );
            }
            StartPlan::RestoreHiddenAtCursor { group } => {
                if let Some(restored) =
                    self.restore_hidden_group_at_cursor(state, key, cursor, parent_boundary, group)
                {
                    return restored;
                }
            }
            StartPlan::RestoreMatchingGroup {
                matched_group,
                retire_conflicting_group_at_cursor,
            } => {
                if retire_conflicting_group_at_cursor {
                    self.retire_conflicting_group_at_cursor(lifecycle, state, key, cursor);
                }
                if let Some(restored) = self.try_restore_matching_group(
                    state,
                    key,
                    cursor,
                    parent_boundary,
                    matched_group,
                ) {
                    return restored;
                }
            }
            StartPlan::InsertFresh {
                retire_conflicting_group_at_cursor,
            } => {
                if retire_conflicting_group_at_cursor {
                    self.retire_conflicting_group_at_cursor(lifecycle, state, key, cursor);
                }
            }
        }

        self.insert_new_group_at_cursor(
            state,
            key,
            PassBoundary::Fresh {
                boundary_key: parent_boundary.inherited_boundary(key),
            },
        )
    }

    fn end_group_entry(&mut self, state: &mut SlotWriteSessionState) {
        let Some(frame) = state.group_stack.pop() else {
            return;
        };

        let end = state.cursor;
        let old_extent = self.storage.group_extent_at(frame.start);
        let new_extent = end.saturating_sub(frame.start);
        let stored_extent = old_extent.max(new_extent).max(1);

        if old_extent > new_extent {
            self.preserve_group_retention(frame.start);
        }

        if stored_extent != old_extent {
            self.storage.set_group_extent(frame.start, stored_extent);
        }

        if new_extent > old_extent {
            self.propagate_group_growth(frame.start, end);
        }

        if let Some(parent) = state.group_stack.last_mut() {
            if parent.end < end {
                parent.end = end;
            }
        }
    }

    fn start_recompose_entry(&self, state: &mut SlotWriteSessionState, index: usize) {
        let Some(group) = self.storage.group_snapshot_at(index) else {
            return;
        };
        state.group_stack.push(GroupFrame {
            start: index,
            end: index + unpack_group_extent(group.extent),
            pass_boundary: PassBoundary::Open,
        });
        state.cursor = index + 1;
    }

    fn end_recompose_entry(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        state: &mut SlotWriteSessionState,
    ) {
        let Some(frame) = state.group_stack.pop() else {
            return;
        };

        let actual_end = state.cursor;
        if actual_end < frame.end {
            let _ = self.mark_range_as_hidden(lifecycle, actual_end, frame.end, Some(frame.start));
        }

        let actual_extent = actual_end.saturating_sub(frame.start).max(1);
        let old_extent = self.storage.group_extent_at(frame.start);
        let stored_extent = old_extent.max(actual_extent);
        if stored_extent != old_extent {
            self.storage.set_group_extent(frame.start, stored_extent);
        }
        if actual_extent > old_extent {
            self.propagate_group_growth(frame.start, actual_end);
        }
        if old_extent > actual_extent {
            self.preserve_group_retention(frame.start);
        }
        state.cursor = actual_end;
    }

    fn propagate_group_growth(&mut self, child_start: usize, new_end: usize) {
        let mut current = child_start;
        while let Some(parent_anchor) = self.storage.group_parent_anchor_at(current) {
            if !parent_anchor.is_valid() {
                break;
            }
            let Some(parent_index) = self.storage.resolve_anchor(parent_anchor) else {
                break;
            };
            let parent_extent = self.storage.group_extent_at(parent_index).max(1);
            let parent_end = parent_index + parent_extent;
            if parent_end < new_end {
                self.storage
                    .set_group_extent(parent_index, new_end.saturating_sub(parent_index));
            }
            current = parent_index;
        }
    }

    fn skip_current(&self, state: &mut SlotWriteSessionState) {
        if let Some(frame) = state.group_stack.last() {
            state.cursor = frame.end.min(self.storage.len());
        }
    }

    fn node_ids_in_current_group(&self, state: &SlotWriteSessionState) -> Vec<NodeId> {
        let Some(frame) = state.group_stack.last() else {
            return Vec::new();
        };

        SlotReadCursor::new(self).collect_node_ids(frame.start, frame.end.min(self.storage.len()))
    }

    #[cfg(test)]
    fn descendant_scopes_in_current_group(
        &self,
        state: &SlotWriteSessionState,
        current_scope: ScopeId,
    ) -> Vec<RecomposeScope> {
        let Some(frame) = state.group_stack.last() else {
            return Vec::new();
        };

        SlotReadCursor::new(self).collect_descendant_scopes(
            frame.start.saturating_add(1),
            frame.end.min(self.storage.len()),
            current_scope,
        )
    }

    fn preserved_hidden_node_at_cursor(
        &self,
        state: &SlotWriteSessionState,
        cursor: usize,
    ) -> Option<(NodeId, u32)> {
        let (kind, id, generation) = self.storage.node_at(cursor)?;
        if kind != EntryKind::HiddenNode
            || !self.current_parent_allows_exact_hidden_node_reuse(state)
        {
            return None;
        }
        Some((id, generation))
    }

    pub(crate) fn read_value<T: 'static>(&self, idx: usize) -> &T {
        self.storage.read_value(idx)
    }

    pub(crate) fn read_value_mut<T: 'static>(&mut self, idx: usize) -> &mut T {
        self.storage.read_value_mut(idx)
    }

    pub(crate) fn write_value<T: 'static>(&mut self, idx: usize, value: T) {
        self.storage.write_value(idx, value);
    }

    pub(crate) fn orphaned_node_state(&self, orphaned: OrphanedNode) -> NodeSlotState {
        self.storage.orphaned_node_state(orphaned)
    }

    pub(crate) fn compact(&mut self) -> Vec<DeferredDrop> {
        self.storage.compact(
            Self::EAGER_COMPACT_SLOT_LEN,
            Self::FRACTIONAL_COMPACT_GAP_THRESHOLD,
            Self::FRACTIONAL_COMPACT_RATIO_DIVISOR,
        )
    }

    fn flush_anchors_if_dirty(&mut self) {
        if self.storage.take_anchors_dirty() {
            self.storage.rebuild_anchor_positions();
        }
    }

    pub(crate) fn flush(&mut self) {
        SlotTable::flush_anchors_if_dirty(self);
    }

    pub(crate) fn drop_all_reverse(&mut self) -> Vec<DeferredDrop> {
        self.storage.drop_all_reverse()
    }
}

impl SlotLifecycleCoordinator {
    pub(crate) fn fill_debug_stats(&self, stats: &mut SlotTableDebugStats) {
        stats.pending_slot_drops_len = self.pending_drops_len();
        stats.pending_slot_drops_cap = self.pending_drops_capacity();
        stats.orphaned_node_ids_len = self.orphaned_node_ids_len();
        stats.orphaned_node_ids_cap = self.orphaned_node_ids_capacity();
    }

    pub(crate) fn drain_orphaned_nodes(
        &mut self,
        table: &SlotTable,
        applier: &mut dyn Applier,
    ) -> bool {
        let orphaned = self.drain_orphaned_node_ids();
        if orphaned.is_empty() {
            return false;
        }

        let mut removed_any = false;
        let mut deferred = Vec::new();
        for orphaned in orphaned {
            match table.orphaned_node_state(orphaned) {
                NodeSlotState::Active => continue,
                NodeSlotState::PreservedGap => {
                    deferred.push(orphaned);
                    continue;
                }
                NodeSlotState::Missing => {}
            }
            if applier.node_generation(orphaned.id) != orphaned.generation {
                continue;
            }
            removed_any = true;
            let parent_id = applier
                .get_mut(orphaned.id)
                .ok()
                .and_then(|node| node.parent());
            if let Some(parent_id) = parent_id {
                let _ = remove_child_and_cleanup_now(applier, parent_id, orphaned.id);
                continue;
            }
            if let Ok(node) = applier.get_mut(orphaned.id) {
                node.on_removed_from_parent();
                node.unmount();
            }
            let _ = applier.remove(orphaned.id);
        }

        for orphaned in deferred {
            self.queue_orphaned_node(orphaned);
        }

        removed_any
    }

    pub(crate) fn dispose_slot_table(&mut self, table: &mut SlotTable) {
        self.flush_pending_drops();
        let removed = table.drop_all_reverse();
        self.dispose_drops_reverse(removed);
        self.trim_orphaned_node_capacity(32);
    }
}

impl<'a> SlotReadCursor<'a> {
    fn new(table: &'a SlotTable) -> Self {
        Self { table }
    }

    fn collect_node_ids(&self, start: usize, end: usize) -> Vec<NodeId> {
        let mut ids = Vec::new();
        for index in start..end {
            if let Some((EntryKind::Node, id, _)) = self.table.storage.node_at(index) {
                ids.push(id);
            }
        }
        ids
    }

    fn collect_group_debug_rows(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        let mut groups = Vec::new();
        for index in 0..self.table.storage.len() {
            if self.table.storage.entry_kind(index) != Some(EntryKind::Group) {
                continue;
            }
            let Some(group) = self.table.storage.group_snapshot_at(index) else {
                continue;
            };
            groups.push((
                index,
                group.key,
                group.scope.as_ref().map(RecomposeScope::id),
                unpack_group_extent(group.extent),
            ));
        }
        groups
    }

    fn collect_slot_debug_rows(&self) -> Vec<(usize, String)> {
        let mut slots = Vec::with_capacity(self.table.storage.len());
        for index in 0..self.table.storage.len() {
            let Some(entry) = self.table.storage.entry(index) else {
                continue;
            };
            let desc = match entry.kind {
                EntryKind::Group => {
                    let group = self
                        .table
                        .storage
                        .group_snapshot_at(index)
                        .expect("live group snapshot");
                    format!(
                        "Group(key={:?}, scope={:?}, has_scope={}, len={})",
                        group.key,
                        group.scope.as_ref().map(RecomposeScope::id),
                        self.table.group_has_scope(index),
                        unpack_group_extent(group.extent)
                    )
                }
                EntryKind::Value => "Value".to_string(),
                EntryKind::Node => {
                    let (_, id, _) = self.table.storage.node_at(index).expect("live node");
                    format!("Node(id={id:?})")
                }
                EntryKind::HiddenGroup => {
                    let group = self
                        .table
                        .storage
                        .group_snapshot_at(index)
                        .expect("hidden group snapshot");
                    format!(
                        "HiddenGroup(key={:?}, scope={:?}, len={})",
                        group.key,
                        group.scope.as_ref().map(RecomposeScope::id),
                        unpack_group_extent(group.extent)
                    )
                }
                EntryKind::HiddenValue => "HiddenValue".to_string(),
                EntryKind::HiddenNode => {
                    let (_, id, generation) =
                        self.table.storage.node_at(index).expect("hidden node");
                    format!("HiddenNode(id={id:?}, gen={generation})")
                }
                EntryKind::Unused => "Unused".to_string(),
            };
            slots.push((index, desc));
        }
        slots
    }

    #[cfg(test)]
    fn collect_descendant_scopes(
        &self,
        start: usize,
        end: usize,
        current_scope: ScopeId,
    ) -> Vec<RecomposeScope> {
        let mut scopes = Vec::new();
        let mut seen = crate::collections::map::HashMap::default();

        for index in start..end {
            let Some(scope) = self.table.storage.live_group_scope(index).cloned() else {
                continue;
            };
            if scope.id() == current_scope || seen.insert(scope.id(), ()).is_some() {
                continue;
            }
            scopes.push(scope);
        }

        scopes
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn start_recranpose_at_anchor(
        &mut self,
        anchor: AnchorId,
        owner: ScopeId,
    ) -> Option<GroupId> {
        let index = self.table.storage.resolve_anchor(anchor)?;
        if self.table.group_scope_owner(index) == Some(owner) {
            self.table.start_recompose_entry(self.state, index);
            Some(GroupId(index))
        } else {
            None
        }
    }

    pub(crate) fn begin_scoped_group(
        &mut self,
        key: Key,
        init_scope: impl FnOnce() -> RecomposeScope,
    ) -> StartScopedGroup<GroupId> {
        let started = self
            .table
            .start_group_entry(self.lifecycle, self.state, key);
        let scope =
            if let Some(existing_scope) = self.table.group_scope_value(started.index).cloned() {
                existing_scope
            } else {
                let scope = init_scope();
                self.table
                    .storage
                    .set_group_scope(started.index, Some(scope.clone()));
                scope
            };
        StartScopedGroup {
            group: GroupId(started.index),
            anchor: self.table.storage.entry_anchor(started.index),
            scope,
            restored_from_gap: started.restored_from_hidden,
        }
    }

    pub(crate) fn end_group(&mut self) {
        self.table.end_group_entry(self.state);
    }

    pub(crate) fn end_recompose(&mut self) {
        self.table.end_recompose_entry(self.lifecycle, self.state);
    }

    pub(crate) fn use_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> usize {
        let cursor = self.state.cursor;
        let disallow_live_reuse = self.table.current_disallow_live_slot_reuse(self.state);

        match self.table.storage.entry_kind(cursor) {
            Some(EntryKind::Value)
                if !disallow_live_reuse && self.table.storage.value_matches_type::<T>(cursor) =>
            {
                self.table.finish_slot_write_at(self.state, cursor)
            }
            Some(EntryKind::HiddenValue)
                if !disallow_live_reuse && self.table.storage.value_matches_type::<T>(cursor) =>
            {
                self.table.storage.restore_hidden_entry(cursor);
                self.table.finish_slot_write_at(self.state, cursor)
            }
            Some(EntryKind::Value | EntryKind::HiddenValue) if !disallow_live_reuse => {
                self.table
                    .overwrite_value_entry(self.lifecycle, cursor, init(), false);
                self.table.finish_slot_write_at(self.state, cursor)
            }
            _ => {
                self.table.insert_value_entry(self.state, cursor, init());
                self.table.finish_slot_write_at(self.state, cursor)
            }
        }
    }

    pub(crate) fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let index = self.use_value_slot(|| Owned::new(init()));
        self.table.read_value::<Owned<T>>(index).clone()
    }

    pub(crate) fn record_node(&mut self, id: NodeId, generation: u32) {
        let cursor = self.state.cursor;
        match self.table.storage.node_at(cursor) {
            Some((EntryKind::Node, existing, existing_generation))
                if existing == id && existing_generation == generation =>
            {
                let _ = self.table.finish_slot_write_at(self.state, cursor);
            }
            Some((EntryKind::HiddenNode, existing, existing_generation))
                if existing == id
                    && existing_generation == generation
                    && self
                        .table
                        .current_parent_allows_exact_hidden_node_reuse(self.state) =>
            {
                self.table.storage.restore_hidden_entry(cursor);
                let _ = self.table.finish_slot_write_at(self.state, cursor);
            }
            Some((EntryKind::Node | EntryKind::HiddenNode, _, _))
                if !self.table.current_disallow_live_slot_reuse(self.state) =>
            {
                self.table
                    .overwrite_node_entry(self.lifecycle, cursor, id, generation, false);
                let _ = self.table.finish_slot_write_at(self.state, cursor);
            }
            _ => {
                self.table
                    .insert_node_entry(self.state, cursor, id, generation);
                let _ = self.table.finish_slot_write_at(self.state, cursor);
            }
        }
    }

    pub(crate) fn peek_node(&self) -> Option<(NodeId, u32)> {
        match self.table.storage.node_at(self.state.cursor) {
            Some((EntryKind::Node, id, generation)) => Some((id, generation)),
            Some((EntryKind::HiddenNode, id, generation))
                if self
                    .table
                    .current_parent_allows_exact_hidden_node_reuse(self.state) =>
            {
                Some((id, generation))
            }
            _ => None,
        }
    }

    pub(crate) fn advance_after_node_read(&mut self) {
        if self
            .table
            .preserved_hidden_node_at_cursor(self.state, self.state.cursor)
            .is_some()
        {
            self.table.storage.restore_hidden_entry(self.state.cursor);
        }
        self.state.cursor += 1;
        self.table.update_group_bounds(self.state);
    }

    pub(crate) fn skip_current_group(&mut self) {
        self.table.skip_current(self.state);
    }

    pub(crate) fn nodes_in_current_group(&self) -> Vec<NodeId> {
        self.table.node_ids_in_current_group(self.state)
    }

    pub(crate) fn finalize_current_group(&mut self) -> bool {
        let mut marked = false;
        if let Some((owner_start, group_end)) = self
            .state
            .group_stack
            .last()
            .map(|frame| (frame.start, frame.end.min(self.table.storage.len())))
        {
            if self.state.cursor < group_end
                && self.table.mark_range_as_hidden(
                    self.lifecycle,
                    self.state.cursor,
                    group_end,
                    Some(owner_start),
                )
            {
                marked = true;
            }
            if let Some(frame) = self.state.group_stack.last_mut() {
                frame.end = self.state.cursor;
            }
        } else if self.state.cursor < self.table.storage.len()
            && self.table.mark_range_as_hidden(
                self.lifecycle,
                self.state.cursor,
                self.table.storage.len(),
                None,
            )
        {
            marked = true;
        }

        marked
    }

    pub(crate) fn finalize_pass(&mut self) -> bool {
        let mut marked_hidden = false;
        while !self.state.group_stack.is_empty() {
            marked_hidden |= self.finalize_current_group();
            match self.mode {
                SlotPassMode::Compose => self.table.end_group_entry(self.state),
                SlotPassMode::Recompose => {
                    self.table.end_recompose_entry(self.lifecycle, self.state)
                }
            }
        }
        match self.mode {
            SlotPassMode::Compose => marked_hidden | self.finalize_current_group(),
            SlotPassMode::Recompose => marked_hidden,
        }
    }
}

#[cfg(test)]
pub(crate) fn begin_group_for_test(
    table: &mut SlotTable,
    state: &mut SlotWriteSessionState,
    key: Key,
) -> GroupId {
    let mut lifecycle = SlotLifecycleCoordinator::default();
    let started = table.start_group_entry(&mut lifecycle, state, key);
    table.clear_group_scope(started.index);
    GroupId(started.index)
}

#[cfg(test)]
pub(crate) fn hide_range_for_test(
    table: &mut SlotTable,
    lifecycle: &mut SlotLifecycleCoordinator,
    start: usize,
    end: usize,
    owner_index: Option<usize>,
) -> bool {
    table.mark_range_as_hidden(lifecycle, start, end, owner_index)
}

#[cfg(test)]
pub(crate) fn queue_orphaned_node_for_test(
    lifecycle: &mut SlotLifecycleCoordinator,
    id: NodeId,
    generation: u32,
) {
    lifecycle.queue_orphaned_node(OrphanedNode::new(id, generation, AnchorId::INVALID));
}

#[cfg(test)]
pub(crate) fn drain_orphaned_node_ids_for_test(
    lifecycle: &mut SlotLifecycleCoordinator,
) -> Vec<OrphanedNode> {
    lifecycle.drain_orphaned_node_ids()
}

#[cfg(test)]
pub(crate) fn compact_for_test(table: &mut SlotTable, lifecycle: &mut SlotLifecycleCoordinator) {
    let removed = table.compact();
    lifecycle.dispose_drops_reverse(removed);
    lifecycle.trim_orphaned_node_capacity(32);
    table.flush();
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}

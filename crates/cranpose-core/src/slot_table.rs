//! Slot table implementation with a single logical gap buffer and explicit hidden entries.

mod anchor_map;
mod boundary_policy;
mod lifecycle_queue;
mod storage;
mod verifier;

use crate::{
    collections::map::HashMap,
    remove_child_and_cleanup_now,
    slot_storage::{GroupId, StartScopedGroup},
    AnchorId, Applier, Key, NodeId, Owned, RecomposeScope, ScopeId,
};
use boundary_policy::{BoundaryTransition, PassBoundary};
use lifecycle_queue::DeferredDrop;
pub use lifecycle_queue::OrphanedNode;
pub(crate) use lifecycle_queue::SlotLifecycleCoordinator;
use storage::{EntryClass, EntryKind, EntryVisibility, GroupSpans, SlotStorage, StorageEffects};
use verifier::debug_verify_slot_table;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableDebugStats {
    pub slots_len: usize,
    pub slots_cap: usize,
    pub pending_slot_drops_len: usize,
    pub pending_slot_drops_cap: usize,
    pub anchors_len: usize,
    pub anchors_cap: usize,
    pub hidden_entries_len: usize,
    pub preserved_groups_len: usize,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotPassMode {
    Compose,
    Recompose,
}

#[derive(Clone, Debug)]
struct GroupFrame {
    start: usize,
    end: usize,
    stored_live_end: usize,
    live_end: usize,
    pass_boundary: PassBoundary,
    previous_direct_child_group: Option<AnchorId>,
}

#[derive(Clone)]
struct MatchedGroup {
    index: usize,
    scan_extent: usize,
    live_extent: usize,
    boundary_key: Key,
    reused_hidden: bool,
}

#[derive(Clone, Copy, Debug)]
struct GroupEntryPlan {
    pass_boundary: PassBoundary,
    requires_recompose: bool,
}

#[derive(Clone, Copy, Debug)]
struct StartedGroup {
    index: usize,
    requires_recompose: bool,
}

#[derive(Default)]
struct DirectChildReparentBatch {
    removed_hidden_descendants: HashMap<usize, usize>,
    removed_preserved_scan: HashMap<usize, usize>,
    added_hidden_descendants: usize,
}

impl DirectChildReparentBatch {
    fn record_previous_parent(
        &mut self,
        previous_parent_index: usize,
        hidden_owned: usize,
        removed_scan_extent: usize,
    ) {
        Self::accumulate(
            &mut self.removed_hidden_descendants,
            previous_parent_index,
            hidden_owned,
        );
        Self::accumulate(
            &mut self.removed_preserved_scan,
            previous_parent_index,
            removed_scan_extent,
        );
    }

    fn add_hidden_descendants_to_new_parent(&mut self, hidden_owned: usize) {
        self.added_hidden_descendants += hidden_owned;
    }

    fn apply(self, table: &mut SlotTable, parent_index: usize) {
        for (previous_parent_index, hidden_owned) in self.removed_hidden_descendants {
            table.adjust_hidden_descendants(Some(previous_parent_index), -(hidden_owned as isize));
        }
        for (previous_parent_index, removed_scan_extent) in self.removed_preserved_scan {
            table.shrink_group_preserved_tail(previous_parent_index, removed_scan_extent);
        }
        if self.added_hidden_descendants > 0 {
            table.adjust_hidden_descendants(
                Some(parent_index),
                self.added_hidden_descendants as isize,
            );
        }
    }

    fn accumulate(map: &mut HashMap<usize, usize>, index: usize, delta: usize) {
        if delta == 0 {
            return;
        }
        *map.entry(index).or_default() += delta;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub(crate) struct GroupRetention {
    boundary_key: Key,
}

impl GroupRetention {
    pub(crate) fn new(boundary_key: Key) -> Self {
        Self { boundary_key }
    }

    pub(crate) fn boundary_key(self) -> Key {
        self.boundary_key
    }
}

#[derive(Default)]
pub(crate) struct SlotWriteSessionState {
    cursor: usize,
    group_stack: Vec<GroupFrame>,
    previous_root_group: Option<AnchorId>,
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

use reuse_planner::{ReusePlanner, ReusePlannerContext, StartPlan};

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
        if let Some(frame) = state.group_stack.last_mut() {
            if frame.end < state.cursor {
                frame.end = state.cursor;
            }
            if frame.live_end < state.cursor {
                frame.live_end = state.cursor;
            }
        }
    }

    fn update_group_live_bounds_to(&self, state: &mut SlotWriteSessionState, live_cursor: usize) {
        if let Some(frame) = state.group_stack.last_mut() {
            if frame.live_end < live_cursor {
                frame.live_end = live_cursor;
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
                    frame.stored_live_end += delta;
                    frame.live_end += delta;
                } else if frame.end >= index {
                    frame.end += delta;
                    if frame.stored_live_end >= index {
                        frame.stored_live_end += delta;
                    }
                    if frame.live_end >= index {
                        frame.live_end += delta;
                    }
                }
            }
        } else {
            let delta = (-delta) as usize;
            for frame in &mut state.group_stack {
                if frame.start >= index {
                    frame.start = frame.start.saturating_sub(delta);
                    frame.end = frame.end.saturating_sub(delta);
                    frame.stored_live_end = frame.stored_live_end.saturating_sub(delta);
                    frame.live_end = frame.live_end.saturating_sub(delta);
                } else if frame.end > index {
                    frame.end = frame.end.saturating_sub(delta);
                    if frame.stored_live_end > index {
                        frame.stored_live_end = frame.stored_live_end.saturating_sub(delta);
                    }
                    if frame.live_end > index {
                        frame.live_end = frame.live_end.saturating_sub(delta);
                    }
                }
            }
        }
    }

    fn finish_slot_write_at(&self, state: &mut SlotWriteSessionState, index: usize) -> usize {
        self.clear_previous_direct_child_group(state);
        state.cursor = index + 1;
        self.update_group_bounds(state);
        index
    }

    fn current_previous_direct_child_group(
        &self,
        state: &SlotWriteSessionState,
    ) -> Option<AnchorId> {
        state
            .group_stack
            .last()
            .and_then(|frame| frame.previous_direct_child_group)
            .or(state.previous_root_group)
    }

    fn clear_previous_direct_child_group(&self, state: &mut SlotWriteSessionState) {
        if let Some(frame) = state.group_stack.last_mut() {
            frame.previous_direct_child_group = None;
        } else {
            state.previous_root_group = None;
        }
    }

    fn set_previous_direct_child_group(&self, state: &mut SlotWriteSessionState, anchor: AnchorId) {
        if let Some(frame) = state.group_stack.last_mut() {
            frame.previous_direct_child_group = Some(anchor);
        } else {
            state.previous_root_group = Some(anchor);
        }
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
            .and_then(|frame| frame.pass_boundary.policy().restricted_boundary())
    }

    fn current_disallow_live_slot_reuse(&self, state: &SlotWriteSessionState) -> bool {
        state
            .group_stack
            .last()
            .is_some_and(|frame| frame.pass_boundary.policy().disallows_live_value_reuse())
    }

    fn current_parent_allows_exact_node_reuse(&self, state: &SlotWriteSessionState) -> bool {
        state
            .group_stack
            .last()
            .map(|frame| frame.pass_boundary.policy().allows_exact_live_reuse())
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

    fn move_slot_range(
        &mut self,
        state: &mut SlotWriteSessionState,
        source_start: usize,
        len: usize,
        dest: usize,
    ) -> usize {
        if len == 0 || source_start == dest {
            return source_start;
        }

        let removed = self.storage.remove_entry_range(source_start, len);
        self.shift_group_frames(state, source_start, -(len as isize));
        let insert_at = if dest > source_start {
            dest.saturating_sub(len)
        } else {
            dest
        }
        .min(self.storage.len());
        self.ensure_insert_capacity(insert_at);
        self.shift_group_frames(state, insert_at, len as isize);
        self.storage.insert_entry_range(insert_at, &removed);
        insert_at
    }

    fn enter_group(
        &self,
        state: &mut SlotWriteSessionState,
        start: usize,
        scan_len: usize,
        live_len: usize,
        plan: GroupEntryPlan,
    ) -> StartedGroup {
        state.group_stack.push(GroupFrame {
            start,
            end: start + scan_len,
            stored_live_end: start + live_len,
            live_end: start + 1,
            pass_boundary: plan.pass_boundary,
            previous_direct_child_group: None,
        });
        state.cursor = start + 1;
        self.update_group_bounds(state);
        StartedGroup {
            index: start,
            requires_recompose: plan.requires_recompose,
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
            GroupRetention::new(boundary_key),
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
        let payload = self.storage.alloc_value(value);
        self.shift_group_frames(state, index, 1);
        self.storage.insert_value(index, payload, false);
    }

    fn overwrite_value_entry<T: 'static>(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        owner_index: Option<usize>,
        index: usize,
        value: T,
        hidden: bool,
    ) {
        if !hidden
            && self
                .storage
                .entry_kind(index)
                .is_some_and(EntryKind::is_hidden)
        {
            self.adjust_hidden_descendants(owner_index, -1);
        }
        let payload = self.storage.alloc_value(value);
        if let Some(deferred_drop) = self.storage.overwrite_value(index, payload, hidden) {
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
        owner_index: Option<usize>,
        index: usize,
        id: NodeId,
        generation: u32,
        hidden: bool,
    ) {
        if !hidden
            && self
                .storage
                .entry_kind(index)
                .is_some_and(EntryKind::is_hidden)
        {
            self.adjust_hidden_descendants(owner_index, -1);
        }
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
        let mut result = self.storage.hide_range(start, end);
        Self::apply_storage_effects(lifecycle, &mut result.effects);
        if result.marked_any {
            let owner_anchor = owner_index
                .map(|index| self.storage.entry_anchor(index))
                .unwrap_or(AnchorId::INVALID);
            for group_index in result.top_level_hidden_group_roots.iter().copied() {
                self.storage
                    .set_group_parent_anchor(group_index, owner_anchor);
            }
            self.adjust_hidden_descendants(owner_index, result.newly_hidden_entries as isize);
            for (group_index, scan_extent) in result.hidden_group_roots {
                self.storage
                    .set_group_hidden_descendants(group_index, scan_extent.saturating_sub(1));
            }
        }
        result.marked_any
    }

    fn adjust_hidden_descendants(&mut self, owner_index: Option<usize>, delta: isize) {
        if delta == 0 {
            return;
        }

        let mut current = owner_index;
        while let Some(index) = current {
            self.storage.adjust_group_hidden_descendants(index, delta);
            current = self
                .storage
                .group_parent_anchor_at(index)
                .filter(|anchor| anchor.is_valid())
                .and_then(|anchor| self.storage.resolve_anchor(anchor));
        }
    }

    fn shrink_group_preserved_tail(&mut self, index: usize, removed_scan_extent: usize) {
        let live_extent = self.storage.group_live_extent_at(index).max(1);
        let scan_extent = self.storage.group_scan_extent_at(index).max(live_extent);
        let next_scan_extent = scan_extent
            .saturating_sub(removed_scan_extent)
            .max(live_extent);
        self.storage
            .set_group_spans(index, GroupSpans::new(live_extent, next_scan_extent));
    }

    fn reparent_direct_child_groups(&mut self, parent_index: usize) {
        let parent_anchor = self.storage.entry_anchor(parent_index);
        let scan_end = parent_index + self.storage.group_scan_extent_at(parent_index).max(1);
        let mut index = parent_index + 1;
        let mut batch = DirectChildReparentBatch::default();

        while index < scan_end {
            let Some(kind) = self.storage.entry_kind(index) else {
                break;
            };
            let extent = self.storage.entry_scan_extent(index).max(1);

            if kind.matches(EntryClass::Group, EntryVisibility::Live)
                || kind.matches(EntryClass::Group, EntryVisibility::Hidden)
            {
                let previous_parent_anchor = self
                    .storage
                    .group_parent_anchor_at(index)
                    .unwrap_or(AnchorId::INVALID);
                if previous_parent_anchor != parent_anchor {
                    let hidden_owned = self.storage.group_hidden_descendants_at(index)
                        + usize::from(kind.matches(EntryClass::Group, EntryVisibility::Hidden));
                    let removed_scan_extent =
                        usize::from(kind.matches(EntryClass::Group, EntryVisibility::Hidden))
                            * extent;

                    if let Some(previous_parent_index) = previous_parent_anchor
                        .is_valid()
                        .then(|| self.storage.resolve_anchor(previous_parent_anchor))
                        .flatten()
                    {
                        batch.record_previous_parent(
                            previous_parent_index,
                            hidden_owned,
                            removed_scan_extent,
                        );
                    }

                    batch.add_hidden_descendants_to_new_parent(hidden_owned);
                    self.storage.set_group_parent_anchor(index, parent_anchor);
                }
            }

            index += extent;
        }

        batch.apply(self, parent_index);
    }

    fn restore_hidden_entry(&mut self, owner_index: Option<usize>, index: usize) {
        let was_hidden = self
            .storage
            .entry_kind(index)
            .is_some_and(EntryKind::is_hidden);
        self.storage.restore_hidden_entry(index);
        if was_hidden {
            self.adjust_hidden_descendants(owner_index, -1);
        }
    }

    fn restore_hidden_group_for_current_parent(
        &mut self,
        state: &SlotWriteSessionState,
        index: usize,
        scan_extent: usize,
    ) {
        let current_parent_anchor = self.current_parent_anchor(state);
        let current_owner_index = state.group_stack.last().map(|frame| frame.start);
        let previous_parent_anchor = self
            .storage
            .group_parent_anchor_at(index)
            .unwrap_or(AnchorId::INVALID);
        let hidden_descendants = self.storage.group_hidden_descendants_at(index);
        let parent_changed = previous_parent_anchor != current_parent_anchor;

        if parent_changed {
            if let Some(previous_parent_index) = previous_parent_anchor
                .is_valid()
                .then(|| self.storage.resolve_anchor(previous_parent_anchor))
                .flatten()
            {
                self.adjust_hidden_descendants(
                    Some(previous_parent_index),
                    -((hidden_descendants + 1) as isize),
                );
                self.shrink_group_preserved_tail(previous_parent_index, scan_extent);
            }
            if hidden_descendants > 0 {
                self.adjust_hidden_descendants(current_owner_index, hidden_descendants as isize);
            }
            self.storage
                .set_group_parent_anchor(index, current_parent_anchor);
        } else {
            self.adjust_hidden_descendants(current_owner_index, -1);
        }

        self.storage.restore_hidden_entry(index);
        if parent_changed {
            self.reparent_direct_child_groups(index);
        }
    }

    fn set_group_boundary_key(&mut self, index: usize, boundary_key: Key) {
        if self.storage.group_retention_at(index).is_some() {
            self.storage
                .set_group_retention(index, GroupRetention::new(boundary_key));
        }
    }

    fn apply_storage_effects(
        lifecycle: &mut SlotLifecycleCoordinator,
        effects: &mut StorageEffects,
    ) {
        for scope in effects.take_deactivated_scopes() {
            lifecycle.deactivate_scope(&scope);
        }
        for orphaned in effects.take_orphaned_nodes() {
            lifecycle.preserve_orphaned_node(orphaned);
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
        if self.storage.entry_kind(cursor) != Some(EntryKind::live(EntryClass::Group))
            || self.storage.group_key_at(cursor) == Some(key)
        {
            return;
        }

        let old_extent = self.storage.group_scan_extent_at(cursor).max(1);
        let owner_index = state.group_stack.last().map(|frame| frame.start);
        let _ = self.mark_range_as_hidden(lifecycle, cursor, cursor + old_extent, owner_index);
    }

    fn restore_hidden_group_at_cursor(
        &mut self,
        state: &mut SlotWriteSessionState,
        cursor: usize,
        parent_boundary: PassBoundary,
        scan_extent: usize,
        live_extent: usize,
        boundary_key: Key,
    ) -> Option<StartedGroup> {
        if self.storage.entry_kind(cursor) != Some(EntryKind::hidden(EntryClass::Group)) {
            return None;
        }

        let pass_boundary =
            parent_boundary.transition(BoundaryTransition::Restore { boundary_key });

        self.restore_hidden_group_for_current_parent(state, cursor, scan_extent);
        self.set_group_boundary_key(cursor, boundary_key);
        Some(self.enter_group(
            state,
            cursor,
            scan_extent,
            live_extent,
            GroupEntryPlan {
                pass_boundary,
                requires_recompose: true,
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
        let current_parent_anchor = self.current_parent_anchor(state);
        if !matched.reused_hidden
            && self
                .storage
                .group_parent_anchor_at(matched.index)
                .unwrap_or(AnchorId::INVALID)
                != current_parent_anchor
        {
            return None;
        }

        let restored_boundary_key =
            if matched.reused_hidden && matches!(parent_boundary, PassBoundary::Fresh { .. }) {
                parent_boundary.policy().inherited_boundary(key)
            } else {
                matched.boundary_key
            };

        if matched.reused_hidden {
            self.restore_hidden_group_for_current_parent(state, matched.index, matched.scan_extent);
            self.set_group_boundary_key(matched.index, restored_boundary_key);
        }

        let requires_recompose = matched.reused_hidden || matched.index != cursor;
        let actual_extent = matched
            .scan_extent
            .max(1)
            .min(self.storage.len().saturating_sub(matched.index));
        if actual_extent == 0 {
            return None;
        }

        let restored_index = self.move_slot_range(state, matched.index, actual_extent, cursor);
        let pass_boundary = if requires_recompose {
            parent_boundary.transition(BoundaryTransition::Restore {
                boundary_key: restored_boundary_key,
            })
        } else {
            parent_boundary.transition(BoundaryTransition::ReuseLive {
                boundary_key: restored_boundary_key,
            })
        };

        Some(self.enter_group(
            state,
            restored_index,
            actual_extent,
            matched.live_extent.min(actual_extent),
            GroupEntryPlan {
                pass_boundary,
                requires_recompose,
            },
        ))
    }

    fn insert_new_group_at_cursor(
        &mut self,
        state: &mut SlotWriteSessionState,
        key: Key,
        pass_boundary: PassBoundary,
    ) -> StartedGroup {
        let boundary_key = pass_boundary.policy().inherited_boundary(key);
        self.insert_group_entry(state, state.cursor, key, boundary_key, None);
        self.enter_group(
            state,
            state.cursor,
            1,
            1,
            GroupEntryPlan {
                pass_boundary,
                requires_recompose: false,
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
            ReusePlannerContext {
                key,
                cursor,
                parent_end: self.current_parent_end(state),
                parent_boundary,
                current_parent_boundary_key: self.current_parent_boundary_key(state),
                current_parent_anchor: self.current_parent_anchor(state),
                previous_live_sibling_group: self.current_previous_direct_child_group(state),
            },
        )
        .plan();

        match plan {
            StartPlan::ReuseLiveAtCursor {
                scan_extent,
                live_extent,
                boundary_key,
            } => {
                let pass_boundary =
                    parent_boundary.transition(BoundaryTransition::ReuseLive { boundary_key });
                return self.enter_group(
                    state,
                    cursor,
                    scan_extent,
                    live_extent,
                    GroupEntryPlan {
                        pass_boundary,
                        requires_recompose: false,
                    },
                );
            }
            StartPlan::RestoreHiddenAtCursor {
                scan_extent,
                live_extent,
                boundary_key,
            } => {
                let boundary_key = if matches!(parent_boundary, PassBoundary::Fresh { .. }) {
                    parent_boundary.policy().inherited_boundary(key)
                } else {
                    boundary_key
                };
                if let Some(restored) = self.restore_hidden_group_at_cursor(
                    state,
                    cursor,
                    parent_boundary,
                    scan_extent,
                    live_extent,
                    boundary_key,
                ) {
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
            parent_boundary.transition(BoundaryTransition::InsertFresh {
                boundary_key: parent_boundary.policy().inherited_boundary(key),
            }),
        )
    }

    fn end_group_entry(&mut self, state: &mut SlotWriteSessionState) {
        let Some(frame) = state.group_stack.pop() else {
            return;
        };

        let completed_anchor = self.storage.entry_anchor(frame.start);
        let scan_end = frame.end;
        let live_end = frame.live_end.max(frame.start + 1);
        let old_live_extent = self.storage.group_live_extent_at(frame.start).max(1);
        let old_scan_extent = self
            .storage
            .group_scan_extent_at(frame.start)
            .max(old_live_extent);
        let new_live_extent = live_end.saturating_sub(frame.start).max(1);
        let new_scan_extent = scan_end
            .saturating_sub(frame.start)
            .max(new_live_extent)
            .max(1);

        self.storage.set_group_spans(
            frame.start,
            GroupSpans::new(new_live_extent, new_scan_extent),
        );
        self.propagate_group_span_delta(
            frame.start,
            new_live_extent as isize - old_live_extent as isize,
            new_scan_extent as isize - old_scan_extent as isize,
        );
        if let Some(parent) = state.group_stack.last_mut() {
            if parent.end < scan_end {
                parent.end = scan_end;
            }
            if parent.live_end < live_end {
                parent.live_end = live_end;
            }
        }
        state.cursor = scan_end.min(self.storage.len());
        self.set_previous_direct_child_group(state, completed_anchor);
    }

    fn start_recompose_entry(&self, state: &mut SlotWriteSessionState, index: usize) {
        let scan_extent = self.storage.group_scan_extent_at(index);
        let live_extent = self.storage.group_live_extent_at(index);
        if scan_extent == 0 || live_extent == 0 {
            return;
        }
        state.group_stack.push(GroupFrame {
            start: index,
            end: index + scan_extent,
            stored_live_end: index + live_extent,
            live_end: index + 1,
            // Restarted recomposition operates inside an already-materialized live subtree.
            // Fresh/restored restrictions only apply while a compose pass is replaying a
            // parent boundary; they must not leak into later scope-local restarts.
            pass_boundary: PassBoundary::Open,
            previous_direct_child_group: None,
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

        let completed_anchor = self.storage.entry_anchor(frame.start);
        let actual_end = state.cursor;
        if actual_end < frame.end {
            let _ = self.mark_range_as_hidden(lifecycle, actual_end, frame.end, Some(frame.start));
        }

        let actual_extent = frame.live_end.saturating_sub(frame.start).max(1);
        let old_live_extent = self.storage.group_live_extent_at(frame.start).max(1);
        let old_scan_extent = self
            .storage
            .group_scan_extent_at(frame.start)
            .max(old_live_extent);
        let new_scan_extent = frame
            .end
            .saturating_sub(frame.start)
            .max(actual_extent)
            .max(1);
        self.storage
            .set_group_spans(frame.start, GroupSpans::new(actual_extent, new_scan_extent));
        self.propagate_group_span_delta(
            frame.start,
            actual_extent as isize - old_live_extent as isize,
            new_scan_extent as isize - old_scan_extent as isize,
        );
        if let Some(parent) = state.group_stack.last_mut() {
            if parent.end < frame.end {
                parent.end = frame.end;
            }
            if parent.live_end < frame.live_end {
                parent.live_end = frame.live_end;
            }
        }
        state.cursor = frame.end.min(self.storage.len());
        self.set_previous_direct_child_group(state, completed_anchor);
    }

    fn propagate_group_span_delta(
        &mut self,
        child_start: usize,
        live_delta: isize,
        scan_delta: isize,
    ) {
        if live_delta == 0 && scan_delta == 0 {
            return;
        }

        let mut current = child_start;
        while let Some(parent_anchor) = self.storage.group_parent_anchor_at(current) {
            if !parent_anchor.is_valid() {
                break;
            }
            let Some(parent_index) = self.storage.resolve_anchor(parent_anchor) else {
                break;
            };
            let parent_scan_extent = self.storage.group_scan_extent_at(parent_index).max(1);
            let parent_live_extent = self.storage.group_live_extent_at(parent_index).max(1);
            let next_live_extent = parent_live_extent.saturating_add_signed(live_delta).max(1);
            let next_scan_extent = parent_scan_extent.saturating_add_signed(scan_delta).max(1);
            self.storage.set_group_spans(
                parent_index,
                GroupSpans::new(next_live_extent, next_scan_extent),
            );
            current = parent_index;
        }
    }

    fn skip_current(&mut self, state: &mut SlotWriteSessionState) {
        if let Some((live_end, scan_end)) = state
            .group_stack
            .last()
            .map(|frame| (frame.stored_live_end, frame.end))
        {
            if let Some(frame) = state.group_stack.last() {
                self.reparent_direct_child_groups(frame.start);
            }
            self.update_group_live_bounds_to(state, live_end.min(self.storage.len()));
            state.cursor = scan_end.min(self.storage.len());
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
        if !kind.matches(EntryClass::Node, EntryVisibility::Hidden)
            || !self.current_parent_allows_exact_node_reuse(state)
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

    pub(crate) fn debug_verify(&self, lifecycle: Option<&SlotLifecycleCoordinator>) {
        debug_verify_slot_table(self, lifecycle);
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
        for orphaned in orphaned {
            match table.orphaned_node_state(orphaned) {
                NodeSlotState::Active => continue,
                NodeSlotState::PreservedGap => {
                    self.preserve_orphaned_node(orphaned);
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

        removed_any
    }

    pub(crate) fn dispose_slot_table(&mut self, table: &mut SlotTable) {
        self.flush_pending_drops();
        let removed = table.drop_all_reverse();
        self.dispose_drops_reverse(removed);
        self.clear_orphaned_nodes();
    }
}

impl<'a> SlotReadCursor<'a> {
    fn new(table: &'a SlotTable) -> Self {
        Self { table }
    }

    fn collect_node_ids(&self, start: usize, end: usize) -> Vec<NodeId> {
        let mut ids = Vec::new();
        for index in start..end {
            if let Some((kind, id, _)) = self.table.storage.node_at(index) {
                if kind.matches(EntryClass::Node, EntryVisibility::Live) {
                    ids.push(id);
                }
            }
        }
        ids
    }

    fn collect_group_debug_rows(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        let mut groups = Vec::new();
        for index in 0..self.table.storage.len() {
            if self.table.storage.entry_kind(index) != Some(EntryKind::live(EntryClass::Group)) {
                continue;
            }
            let Some(group) = self.table.storage.group_snapshot_at(index) else {
                continue;
            };
            groups.push((
                index,
                group.key,
                group.scope.as_ref().map(RecomposeScope::id),
                group.spans.live_extent(),
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
                kind if kind.matches(EntryClass::Group, EntryVisibility::Live) => {
                    let group = self
                        .table
                        .storage
                        .group_snapshot_at(index)
                        .expect("live group snapshot");
                    format!(
                        "Group(key={:?}, scope={:?}, has_scope={}, live_len={}, scan_len={})",
                        group.key,
                        group.scope.as_ref().map(RecomposeScope::id),
                        self.table.group_has_scope(index),
                        group.spans.live_extent(),
                        group.spans.scan_extent(),
                    )
                }
                kind if kind.matches(EntryClass::Value, EntryVisibility::Live) => {
                    "Value".to_string()
                }
                kind if kind.matches(EntryClass::Node, EntryVisibility::Live) => {
                    let (_, id, _) = self.table.storage.node_at(index).expect("live node");
                    format!("Node(id={id:?})")
                }
                kind if kind.matches(EntryClass::Group, EntryVisibility::Hidden) => {
                    let group = self
                        .table
                        .storage
                        .group_snapshot_at(index)
                        .expect("hidden group snapshot");
                    format!(
                        "HiddenGroup(key={:?}, scope={:?}, live_len={}, scan_len={})",
                        group.key,
                        group.scope.as_ref().map(RecomposeScope::id),
                        group.spans.live_extent(),
                        group.spans.scan_extent(),
                    )
                }
                kind if kind.matches(EntryClass::Value, EntryVisibility::Hidden) => {
                    "HiddenValue".to_string()
                }
                kind if kind.matches(EntryClass::Node, EntryVisibility::Hidden) => {
                    let (_, id, generation) =
                        self.table.storage.node_at(index).expect("hidden node");
                    format!("HiddenNode(id={id:?}, gen={generation})")
                }
                EntryKind::Occupied { .. } => "Occupied".to_string(),
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
        let mut seen: crate::collections::map::HashMap<ScopeId, ()> =
            crate::collections::map::HashMap::default();

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
            requires_recompose: started.requires_recompose,
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
        let owner_index = self.state.group_stack.last().map(|frame| frame.start);

        match self.table.storage.entry_kind(cursor) {
            Some(kind)
                if kind.matches(EntryClass::Value, EntryVisibility::Live)
                    && !disallow_live_reuse
                    && self.table.storage.value_matches_type::<T>(cursor) =>
            {
                self.table.finish_slot_write_at(self.state, cursor)
            }
            Some(kind)
                if kind.matches(EntryClass::Value, EntryVisibility::Hidden)
                    && !disallow_live_reuse
                    && self.table.storage.value_matches_type::<T>(cursor) =>
            {
                self.table.restore_hidden_entry(owner_index, cursor);
                self.table.finish_slot_write_at(self.state, cursor)
            }
            Some(EntryKind::Occupied {
                class: EntryClass::Value,
                ..
            }) if !disallow_live_reuse => {
                self.table.overwrite_value_entry(
                    self.lifecycle,
                    owner_index,
                    cursor,
                    init(),
                    false,
                );
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
        let owner_index = self.state.group_stack.last().map(|frame| frame.start);
        match self.table.storage.node_at(cursor) {
            Some((kind, existing, existing_generation))
                if kind.matches(EntryClass::Node, EntryVisibility::Live)
                    && existing == id
                    && existing_generation == generation =>
            {
                let _ = self.table.finish_slot_write_at(self.state, cursor);
            }
            Some((kind, existing, existing_generation))
                if kind.matches(EntryClass::Node, EntryVisibility::Hidden)
                    && existing == id
                    && existing_generation == generation =>
            {
                if self
                    .table
                    .current_parent_allows_exact_node_reuse(self.state)
                {
                    self.table.restore_hidden_entry(owner_index, cursor);
                    let _ = self.table.finish_slot_write_at(self.state, cursor);
                } else if !self.table.current_disallow_live_slot_reuse(self.state) {
                    self.table.overwrite_node_entry(
                        self.lifecycle,
                        owner_index,
                        cursor,
                        id,
                        generation,
                        false,
                    );
                    let _ = self.table.finish_slot_write_at(self.state, cursor);
                } else {
                    self.table
                        .insert_node_entry(self.state, cursor, id, generation);
                    let _ = self.table.finish_slot_write_at(self.state, cursor);
                }
            }
            Some((
                EntryKind::Occupied {
                    class: EntryClass::Node,
                    ..
                },
                _,
                _,
            )) if !self.table.current_disallow_live_slot_reuse(self.state) => {
                self.table.overwrite_node_entry(
                    self.lifecycle,
                    owner_index,
                    cursor,
                    id,
                    generation,
                    false,
                );
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
            Some((kind, id, generation))
                if kind.matches(EntryClass::Node, EntryVisibility::Live) =>
            {
                Some((id, generation))
            }
            Some((kind, id, generation))
                if kind.matches(EntryClass::Node, EntryVisibility::Hidden)
                    && self
                        .table
                        .current_parent_allows_exact_node_reuse(self.state) =>
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
            let owner_index = self.state.group_stack.last().map(|frame| frame.start);
            self.table
                .restore_hidden_entry(owner_index, self.state.cursor);
        }
        self.table.clear_previous_direct_child_group(self.state);
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
pub(crate) fn preserved_orphaned_node_ids_for_test(
    lifecycle: &SlotLifecycleCoordinator,
) -> Vec<OrphanedNode> {
    lifecycle.preserved_orphaned_node_ids_snapshot()
}

#[cfg(test)]
pub(crate) fn compact_for_test(table: &mut SlotTable, lifecycle: &mut SlotLifecycleCoordinator) {
    let removed = table.compact();
    lifecycle.dispose_drops_reverse(removed);
    lifecycle.trim_orphaned_node_capacity(32);
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}

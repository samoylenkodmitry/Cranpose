//! Slot table implementation with a single logical gap buffer and explicit hidden entries.

mod anchor_map;
mod lifecycle_queue;
mod storage;

use crate::{
    slot_storage::{GroupId, StartScopedGroup},
    AnchorId, Key, NodeId, Owned, RecomposeScope, ScopeId,
};
pub use lifecycle_queue::OrphanedNode;
use storage::{EntryKind, GroupSnapshot, SlotStorage};

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

fn unpack_slot_len(len: u32) -> usize {
    len as usize
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReuseState {
    Normal,
    Restored { boundary_key: Key },
    Fresh { boundary_key: Key },
}

impl ReuseState {
    fn restricted_boundary(self) -> Option<Key> {
        match self {
            Self::Normal => None,
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
        matches!(self, Self::Normal)
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

#[derive(Clone, Debug)]
struct GroupFrame {
    start: usize,
    end: usize,
    reuse_state: ReuseState,
}

#[derive(Clone)]
struct MatchedGroup {
    index: usize,
    group: GroupSnapshot,
    reused_hidden: bool,
}

#[derive(Clone, Copy, Debug)]
struct GroupEntryPlan {
    reuse_state: ReuseState,
    restored_from_hidden: bool,
}

#[derive(Clone, Copy, Debug)]
struct StartedGroup {
    index: usize,
    restored_from_hidden: bool,
}

mod reuse_planner;
#[cfg(test)]
mod tests;

use reuse_planner::{ReusePlanner, StartPlan};

pub struct SlotTable {
    storage: SlotStorage,
    cursor: usize,
    group_stack: Vec<GroupFrame>,
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
            cursor: 0,
            group_stack: Vec::new(),
        }
    }

    pub fn heap_bytes(&self) -> usize {
        self.storage.heap_bytes() + self.group_stack.capacity() * std::mem::size_of::<GroupFrame>()
    }

    pub fn debug_stats(&self) -> SlotTableDebugStats {
        let mut stats = self.storage.debug_stats();
        stats.group_stack_len = self.group_stack.len();
        stats.group_stack_cap = self.group_stack.capacity();
        stats
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

    fn update_group_bounds(&mut self) {
        for frame in &mut self.group_stack {
            if frame.end < self.cursor {
                frame.end = self.cursor;
            }
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

    fn finish_slot_write_at(&mut self, index: usize) -> usize {
        self.cursor = index + 1;
        self.update_group_bounds();
        index
    }

    fn current_parent_reuse(&self) -> ReuseState {
        self.group_stack
            .last()
            .map(|frame| frame.reuse_state)
            .unwrap_or(ReuseState::Normal)
    }

    fn current_parent_end(&self) -> usize {
        self.group_stack
            .last()
            .map(|frame| frame.end.min(self.storage.len()))
            .unwrap_or(self.storage.len())
    }

    fn current_parent_anchor(&self) -> AnchorId {
        self.group_stack
            .last()
            .map(|frame| self.storage.entry_anchor(frame.start))
            .unwrap_or(AnchorId::INVALID)
    }

    fn current_parent_boundary_key(&self) -> Option<Key> {
        self.group_stack
            .last()
            .and_then(|frame| frame.reuse_state.restricted_boundary())
    }

    fn current_disallow_live_slot_reuse(&self) -> bool {
        self.group_stack
            .last()
            .is_some_and(|frame| frame.reuse_state.disallows_live_value_reuse())
    }

    fn current_parent_allows_exact_hidden_node_reuse(&self) -> bool {
        self.group_stack
            .last()
            .map(|frame| frame.reuse_state.allows_exact_live_reuse())
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

    fn move_slot_range_to_cursor(&mut self, source_start: usize, len: usize, dest: usize) {
        debug_assert!(dest <= source_start, "unexpected rightward slot move");
        if len == 0 || source_start == dest {
            return;
        }

        let removed = self.storage.remove_entry_range(source_start, len);
        self.shift_group_frames(source_start, -(len as isize));
        self.ensure_insert_capacity(dest);
        self.shift_group_frames(dest, len as isize);
        self.storage.insert_entry_range(dest, &removed);
    }

    fn enter_group(&mut self, start: usize, len: usize, plan: GroupEntryPlan) -> StartedGroup {
        self.group_stack.push(GroupFrame {
            start,
            end: start + len,
            reuse_state: plan.reuse_state,
        });
        self.cursor = start + 1;
        self.update_group_bounds();
        StartedGroup {
            index: start,
            restored_from_hidden: plan.restored_from_hidden,
        }
    }

    fn insert_group_entry(
        &mut self,
        index: usize,
        key: Key,
        boundary_key: Key,
        scope: Option<RecomposeScope>,
    ) -> AnchorId {
        self.ensure_insert_capacity(index);
        let anchor = self.storage.allocate_anchor();
        let group =
            self.storage
                .alloc_group(key, boundary_key, self.current_parent_anchor(), scope);
        self.shift_group_frames(index, 1);
        self.storage.insert_group(index, anchor, group, false);
        anchor
    }

    fn insert_value_entry<T: 'static>(&mut self, index: usize, value: T) {
        self.ensure_insert_capacity(index);
        let anchor = self.storage.allocate_anchor();
        let payload = self.storage.alloc_value(value);
        self.shift_group_frames(index, 1);
        self.storage.insert_value(index, anchor, payload, false);
    }

    fn overwrite_value_entry<T: 'static>(&mut self, index: usize, value: T, hidden: bool) {
        let anchor = self.storage.entry_anchor(index);
        let payload = self.storage.alloc_value(value);
        self.storage.overwrite_value(index, anchor, payload, hidden);
    }

    fn insert_node_entry(&mut self, index: usize, id: NodeId, generation: u32) {
        self.ensure_insert_capacity(index);
        let anchor = self.storage.allocate_anchor();
        let payload = self.storage.alloc_node(id, generation);
        self.shift_group_frames(index, 1);
        self.storage.insert_node(index, anchor, payload, false);
    }

    fn overwrite_node_entry(&mut self, index: usize, id: NodeId, generation: u32, hidden: bool) {
        let anchor = self.storage.entry_anchor(index);
        let payload = self.storage.alloc_node(id, generation);
        self.storage.overwrite_node(index, anchor, payload, hidden);
    }

    fn mark_range_as_hidden(
        &mut self,
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
                        self.storage.hide_entry(child);
                        marked_any = true;
                    }
                    if subtree_end > index + 1 {
                        self.storage.set_group_has_hidden_children(index, true);
                    }
                    index = subtree_end;
                }
                EntryKind::Value | EntryKind::Node => {
                    self.storage.hide_entry(index);
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
                self.storage
                    .set_group_has_hidden_children(owner_index, true);
            }
        }

        marked_any
    }

    pub(crate) fn start_recranpose_at_anchor(
        &mut self,
        anchor: AnchorId,
        owner: ScopeId,
    ) -> Option<GroupId> {
        let index = self.storage.resolve_anchor(anchor)?;
        if self.group_scope_owner(index) == Some(owner) {
            self.start_recompose(index);
            Some(GroupId(index))
        } else {
            None
        }
    }

    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        let mut groups = Vec::new();
        for index in 0..self.storage.len() {
            if self.storage.entry_kind(index) != Some(EntryKind::Group) {
                continue;
            }
            let Some(group) = self.storage.group_snapshot_at(index) else {
                continue;
            };
            groups.push((
                index,
                group.key,
                group.scope.as_ref().map(RecomposeScope::id),
                unpack_slot_len(group.len),
            ));
        }
        groups
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        let mut slots = Vec::with_capacity(self.storage.len());
        for index in 0..self.storage.len() {
            let Some(entry) = self.storage.entry(index) else {
                continue;
            };
            let desc = match entry.kind {
                EntryKind::Group => {
                    let group = self
                        .storage
                        .group_snapshot_at(index)
                        .expect("live group snapshot");
                    format!(
                        "Group(key={:?}, scope={:?}, has_scope={}, len={})",
                        group.key,
                        group.scope.as_ref().map(RecomposeScope::id),
                        self.group_has_scope(index),
                        unpack_slot_len(group.len)
                    )
                }
                EntryKind::Value => "Value".to_string(),
                EntryKind::Node => {
                    let (_, id, _) = self.storage.node_at(index).expect("live node");
                    format!("Node(id={id:?})")
                }
                EntryKind::HiddenGroup => {
                    let group = self
                        .storage
                        .group_snapshot_at(index)
                        .expect("hidden group snapshot");
                    format!(
                        "HiddenGroup(key={:?}, scope={:?}, len={})",
                        group.key,
                        group.scope.as_ref().map(RecomposeScope::id),
                        unpack_slot_len(group.len)
                    )
                }
                EntryKind::HiddenValue => "HiddenValue".to_string(),
                EntryKind::HiddenNode => {
                    let (_, id, generation) = self.storage.node_at(index).expect("hidden node");
                    format!("HiddenNode(id={id:?}, gen={generation})")
                }
                EntryKind::Unused => "Unused".to_string(),
            };
            slots.push((index, desc));
        }
        slots
    }

    pub fn debug_value_type_counts(&self, limit: usize) -> Vec<SlotValueTypeDebugStat> {
        self.storage.debug_value_type_counts(limit)
    }

    fn retire_conflicting_group_at_cursor(&mut self, key: Key, cursor: usize) {
        let Some(group) = self.storage.group_snapshot_at(cursor) else {
            return;
        };
        if self.storage.entry_kind(cursor) != Some(EntryKind::Group) || group.key == key {
            return;
        }

        let old_len = unpack_slot_len(group.len).max(1);
        let owner_index = self.group_stack.last().map(|frame| frame.start);
        let _ = self.mark_range_as_hidden(cursor, cursor + old_len, owner_index);
    }

    fn restore_hidden_group_at_cursor(
        &mut self,
        key: Key,
        cursor: usize,
        parent_reuse: ReuseState,
        group: GroupSnapshot,
    ) -> Option<StartedGroup> {
        if self.storage.entry_kind(cursor) != Some(EntryKind::HiddenGroup) || group.key != key {
            return None;
        }

        let boundary_key = if matches!(parent_reuse, ReuseState::Fresh { .. }) {
            parent_reuse.inherited_boundary(key)
        } else {
            group.boundary_key
        };
        let reuse_state = parent_reuse.child_for_restored(boundary_key);

        self.storage.restore_hidden_entry(cursor);
        self.storage.set_group_boundary_key(cursor, boundary_key);
        Some(self.enter_group(
            cursor,
            unpack_slot_len(group.len),
            GroupEntryPlan {
                reuse_state,
                restored_from_hidden: true,
            },
        ))
    }

    fn try_restore_matching_group(
        &mut self,
        key: Key,
        cursor: usize,
        parent_reuse: ReuseState,
        matched: MatchedGroup,
    ) -> Option<StartedGroup> {
        let restored_boundary_key =
            if matched.reused_hidden && matches!(parent_reuse, ReuseState::Fresh { .. }) {
                parent_reuse.inherited_boundary(key)
            } else {
                matched.group.boundary_key
            };

        if matched.reused_hidden {
            self.storage.restore_hidden_entry(matched.index);
            self.storage
                .set_group_boundary_key(matched.index, restored_boundary_key);
        }

        let restored_from_hidden = matched.reused_hidden || matched.index != cursor;
        let actual_len = unpack_slot_len(matched.group.len)
            .max(1)
            .min(self.storage.len().saturating_sub(matched.index));
        if actual_len == 0 {
            return None;
        }

        self.move_slot_range_to_cursor(matched.index, actual_len, cursor);
        let reuse_state = if restored_from_hidden {
            parent_reuse.child_for_restored(restored_boundary_key)
        } else {
            match parent_reuse {
                ReuseState::Normal => ReuseState::Normal,
                ReuseState::Restored { .. } => ReuseState::Restored {
                    boundary_key: restored_boundary_key,
                },
                ReuseState::Fresh { .. } => ReuseState::Fresh {
                    boundary_key: restored_boundary_key,
                },
            }
        };

        Some(self.enter_group(
            cursor,
            actual_len,
            GroupEntryPlan {
                reuse_state,
                restored_from_hidden,
            },
        ))
    }

    fn insert_new_group_at_cursor(&mut self, key: Key, reuse_state: ReuseState) -> StartedGroup {
        let boundary_key = reuse_state.inherited_boundary(key);
        self.insert_group_entry(self.cursor, key, boundary_key, None);
        self.enter_group(
            self.cursor,
            1,
            GroupEntryPlan {
                reuse_state,
                restored_from_hidden: false,
            },
        )
    }

    fn start_group_entry(&mut self, key: Key) -> StartedGroup {
        let cursor = self.cursor;
        let parent_reuse = self.current_parent_reuse();

        let plan = ReusePlanner::new(
            &self.storage,
            key,
            cursor,
            self.current_parent_end(),
            parent_reuse,
            self.current_parent_boundary_key(),
        )
        .plan();

        match plan {
            StartPlan::ReuseLiveAtCursor { len, boundary_key } => {
                let reuse_state = match parent_reuse {
                    ReuseState::Normal => ReuseState::Normal,
                    ReuseState::Restored { .. } => ReuseState::Restored { boundary_key },
                    ReuseState::Fresh { .. } => ReuseState::Fresh { boundary_key },
                };
                return self.enter_group(
                    cursor,
                    len,
                    GroupEntryPlan {
                        reuse_state,
                        restored_from_hidden: !matches!(reuse_state, ReuseState::Normal),
                    },
                );
            }
            StartPlan::RestoreHiddenAtCursor { group } => {
                if let Some(restored) =
                    self.restore_hidden_group_at_cursor(key, cursor, parent_reuse, group)
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

        self.insert_new_group_at_cursor(
            key,
            ReuseState::Fresh {
                boundary_key: parent_reuse.inherited_boundary(key),
            },
        )
    }

    fn end(&mut self) {
        let Some(frame) = self.group_stack.pop() else {
            return;
        };

        let end = self.cursor;
        let old_len = self.storage.group_len_at(frame.start);
        let new_len = end.saturating_sub(frame.start);
        let stored_len = old_len.max(new_len).max(1);

        if old_len > new_len {
            self.storage
                .set_group_has_hidden_children(frame.start, true);
        }

        if stored_len != old_len {
            self.storage.set_group_len(frame.start, stored_len);
        }

        if new_len > old_len {
            self.propagate_group_growth(frame.start, end);
        }

        if let Some(parent) = self.group_stack.last_mut() {
            if parent.end < end {
                parent.end = end;
            }
        }
    }

    fn start_recompose(&mut self, index: usize) {
        let Some(group) = self.storage.group_snapshot_at(index) else {
            return;
        };
        self.group_stack.push(GroupFrame {
            start: index,
            end: index + unpack_slot_len(group.len),
            reuse_state: ReuseState::Normal,
        });
        self.cursor = index + 1;
    }

    pub fn end_recompose(&mut self) {
        let Some(frame) = self.group_stack.pop() else {
            return;
        };

        let actual_end = self.cursor;
        if actual_end < frame.end {
            let _ = self.mark_range_as_hidden(actual_end, frame.end, Some(frame.start));
        }

        let actual_len = actual_end.saturating_sub(frame.start).max(1);
        let old_len = self.storage.group_len_at(frame.start);
        let stored_len = old_len.max(actual_len);
        if stored_len != old_len {
            self.storage.set_group_len(frame.start, stored_len);
        }
        if actual_len > old_len {
            self.propagate_group_growth(frame.start, actual_end);
        }
        self.cursor = actual_end;
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
            let parent_len = self.storage.group_len_at(parent_index).max(1);
            let parent_end = parent_index + parent_len;
            if parent_end < new_end {
                self.storage
                    .set_group_len(parent_index, new_end.saturating_sub(parent_index));
            }
            current = parent_index;
        }
    }

    fn skip_current(&mut self) {
        if let Some(frame) = self.group_stack.last() {
            self.cursor = frame.end.min(self.storage.len());
        }
    }

    fn node_ids_in_current_group(&self) -> Vec<NodeId> {
        let Some(frame) = self.group_stack.last() else {
            return Vec::new();
        };

        let mut ids = Vec::new();
        let end = frame.end.min(self.storage.len());
        for index in frame.start..end {
            if let Some((EntryKind::Node, id, _)) = self.storage.node_at(index) {
                ids.push(id);
            }
        }
        ids
    }

    #[cfg(test)]
    fn descendant_scopes_in_current_group(&self, current_scope: ScopeId) -> Vec<RecomposeScope> {
        let Some(frame) = self.group_stack.last() else {
            return Vec::new();
        };

        let end = frame.end.min(self.storage.len());
        let mut scopes = Vec::new();
        let mut seen = crate::collections::map::HashMap::default();

        for index in frame.start.saturating_add(1)..end {
            let Some(scope) = self.storage.live_group_scope(index).cloned() else {
                continue;
            };
            if scope.id() == current_scope || seen.insert(scope.id(), ()).is_some() {
                continue;
            }
            scopes.push(scope);
        }

        scopes
    }

    fn preserved_hidden_node_at_cursor(&self, cursor: usize) -> Option<(NodeId, u32)> {
        let (kind, id, generation) = self.storage.node_at(cursor)?;
        if kind != EntryKind::HiddenNode || !self.current_parent_allows_exact_hidden_node_reuse() {
            return None;
        }
        Some((id, generation))
    }

    pub fn use_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> usize {
        let cursor = self.cursor;
        let disallow_live_reuse = self.current_disallow_live_slot_reuse();

        match self.storage.entry_kind(cursor) {
            Some(EntryKind::Value)
                if !disallow_live_reuse && self.storage.value_matches_type::<T>(cursor) =>
            {
                self.finish_slot_write_at(cursor)
            }
            Some(EntryKind::HiddenValue)
                if !disallow_live_reuse && self.storage.value_matches_type::<T>(cursor) =>
            {
                self.storage.restore_hidden_entry(cursor);
                self.finish_slot_write_at(cursor)
            }
            Some(EntryKind::Value | EntryKind::HiddenValue) if !disallow_live_reuse => {
                self.overwrite_value_entry(cursor, init(), false);
                self.finish_slot_write_at(cursor)
            }
            _ => {
                self.insert_value_entry(cursor, init());
                self.finish_slot_write_at(cursor)
            }
        }
    }

    pub fn read_value<T: 'static>(&self, idx: usize) -> &T {
        self.storage.read_value(idx)
    }

    pub fn read_value_mut<T: 'static>(&mut self, idx: usize) -> &mut T {
        self.storage.read_value_mut(idx)
    }

    pub fn write_value<T: 'static>(&mut self, idx: usize, value: T) {
        self.storage.write_value(idx, value);
    }

    pub fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let index = self.use_value_slot(|| Owned::new(init()));
        self.read_value::<Owned<T>>(index).clone()
    }

    pub fn record_node(&mut self, id: NodeId, generation: u32) {
        let cursor = self.cursor;
        match self.storage.node_at(cursor) {
            Some((EntryKind::Node, existing, existing_generation))
                if existing == id && existing_generation == generation =>
            {
                let _ = self.finish_slot_write_at(cursor);
            }
            Some((EntryKind::HiddenNode, existing, existing_generation))
                if existing == id
                    && existing_generation == generation
                    && self.current_parent_allows_exact_hidden_node_reuse() =>
            {
                self.storage.restore_hidden_entry(cursor);
                let _ = self.finish_slot_write_at(cursor);
            }
            Some((EntryKind::Node | EntryKind::HiddenNode, _, _))
                if !self.current_disallow_live_slot_reuse() =>
            {
                self.overwrite_node_entry(cursor, id, generation, false);
                let _ = self.finish_slot_write_at(cursor);
            }
            _ => {
                self.insert_node_entry(cursor, id, generation);
                let _ = self.finish_slot_write_at(cursor);
            }
        }
    }

    pub fn peek_node(&self) -> Option<(NodeId, u32)> {
        match self.storage.node_at(self.cursor) {
            Some((EntryKind::Node, id, generation)) => Some((id, generation)),
            Some((EntryKind::HiddenNode, id, generation))
                if self.current_parent_allows_exact_hidden_node_reuse() =>
            {
                Some((id, generation))
            }
            _ => None,
        }
    }

    pub fn read_node(&mut self) -> Option<NodeId> {
        let node = match self.storage.node_at(self.cursor) {
            Some((EntryKind::Node, id, _)) => Some(id),
            _ => None,
        };
        if node.is_some() {
            self.cursor += 1;
            self.update_group_bounds();
        }
        node
    }

    pub fn advance_after_node_read(&mut self) {
        if self.preserved_hidden_node_at_cursor(self.cursor).is_some() {
            self.storage.restore_hidden_entry(self.cursor);
        }
        self.cursor += 1;
        self.update_group_bounds();
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
        self.group_stack.clear();
    }

    pub fn trim_to_cursor(&mut self) -> bool {
        let mut marked = false;
        if let Some((owner_start, group_end)) = self
            .group_stack
            .last()
            .map(|frame| (frame.start, frame.end.min(self.storage.len())))
        {
            if self.cursor < group_end
                && self.mark_range_as_hidden(self.cursor, group_end, Some(owner_start))
            {
                marked = true;
            }
            if let Some(frame) = self.group_stack.last_mut() {
                frame.end = self.cursor;
            }
        } else if self.cursor < self.storage.len()
            && self.mark_range_as_hidden(self.cursor, self.storage.len(), None)
        {
            marked = true;
        }

        self.storage.flush_pending_drops();
        marked
    }

    pub fn drain_orphaned_node_ids(&mut self) -> Vec<OrphanedNode> {
        self.storage.drain_orphaned_node_ids()
    }

    pub(crate) fn requeue_orphaned_node(&mut self, orphaned: OrphanedNode) {
        self.storage.requeue_orphaned_node(orphaned);
    }

    pub(crate) fn orphaned_node_state(&self, orphaned: OrphanedNode) -> NodeSlotState {
        self.storage.orphaned_node_state(orphaned)
    }

    pub fn compact(&mut self) {
        self.storage.compact(
            Self::EAGER_COMPACT_SLOT_LEN,
            Self::FRACTIONAL_COMPACT_GAP_THRESHOLD,
            Self::FRACTIONAL_COMPACT_RATIO_DIVISOR,
        );
    }

    fn flush_anchors_if_dirty(&mut self) {
        if self.storage.take_anchors_dirty() {
            self.storage.rebuild_anchor_positions();
        }
    }

    pub fn begin_scoped_group(
        &mut self,
        key: Key,
        init_scope: impl FnOnce() -> RecomposeScope,
    ) -> StartScopedGroup<GroupId> {
        let started = self.start_group_entry(key);
        let scope = if let Some(existing_scope) = self.group_scope_value(started.index).cloned() {
            existing_scope
        } else {
            let scope = init_scope();
            self.storage
                .set_group_scope(started.index, Some(scope.clone()));
            scope
        };
        StartScopedGroup {
            group: GroupId(started.index),
            anchor: self.storage.entry_anchor(started.index),
            scope,
            restored_from_gap: started.restored_from_hidden,
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

#[cfg(test)]
pub(crate) fn begin_group_for_test(table: &mut SlotTable, key: Key) -> GroupId {
    let started = table.start_group_entry(key);
    table.clear_group_scope(started.index);
    GroupId(started.index)
}

#[cfg(test)]
pub(crate) fn hide_range_for_test(
    table: &mut SlotTable,
    start: usize,
    end: usize,
    owner_index: Option<usize>,
) -> bool {
    table.mark_range_as_hidden(start, end, owner_index)
}

#[cfg(test)]
pub(crate) fn queue_orphaned_node_for_test(table: &mut SlotTable, id: NodeId, generation: u32) {
    table
        .storage
        .requeue_orphaned_node(OrphanedNode::new(id, generation, AnchorId::INVALID));
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SlotTable {
    fn drop(&mut self) {
        self.storage.flush_pending_drops();
        self.storage.drop_all_reverse();
    }
}

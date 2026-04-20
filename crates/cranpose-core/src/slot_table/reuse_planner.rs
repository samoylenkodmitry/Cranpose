use crate::{AnchorId, Key};

use super::{
    boundary_policy::PassBoundary,
    storage::{EntryClass, EntryKind, EntryVisibility, SlotStorage},
    MatchedGroup,
};

pub(super) enum StartPlan {
    ReuseLiveAtCursor {
        scan_extent: usize,
        live_extent: usize,
        boundary_key: Key,
    },
    RestoreHiddenAtCursor {
        scan_extent: usize,
        live_extent: usize,
        boundary_key: Key,
    },
    RestoreMatchingGroup {
        matched_group: MatchedGroup,
        retire_conflicting_group_at_cursor: bool,
    },
    InsertFresh {
        retire_conflicting_group_at_cursor: bool,
    },
}

pub(super) struct ReusePlannerContext {
    pub(super) key: Key,
    pub(super) cursor: usize,
    pub(super) parent_end: usize,
    pub(super) parent_boundary: PassBoundary,
    pub(super) current_parent_boundary_key: Option<Key>,
    pub(super) current_parent_anchor: AnchorId,
    pub(super) previous_live_sibling_group: Option<AnchorId>,
}

pub(super) struct ReusePlanner<'a> {
    storage: &'a SlotStorage,
    key: Key,
    cursor: usize,
    parent_end: usize,
    parent_boundary: PassBoundary,
    current_parent_boundary_key: Option<Key>,
    current_parent_anchor: AnchorId,
    previous_live_sibling_group: Option<AnchorId>,
}

impl<'a> ReusePlanner<'a> {
    pub(super) fn new(storage: &'a SlotStorage, context: ReusePlannerContext) -> Self {
        let ReusePlannerContext {
            key,
            cursor,
            parent_end,
            parent_boundary,
            current_parent_boundary_key,
            current_parent_anchor,
            previous_live_sibling_group,
        } = context;
        Self {
            storage,
            key,
            cursor,
            parent_end,
            parent_boundary,
            current_parent_boundary_key,
            current_parent_anchor,
            previous_live_sibling_group,
        }
    }

    pub(super) fn plan(&self) -> StartPlan {
        if let Some(matched_group) = self.find_matching_hidden_group_in_previous_tail() {
            return StartPlan::RestoreMatchingGroup {
                matched_group,
                retire_conflicting_group_at_cursor: false,
            };
        }

        if self.cursor >= self.parent_end {
            return StartPlan::InsertFresh {
                retire_conflicting_group_at_cursor: false,
            };
        }

        if let Some(plan) = self.try_reuse_live_at_cursor(false) {
            return plan;
        }

        if self.previous_live_group_has_same_key() {
            return StartPlan::InsertFresh {
                retire_conflicting_group_at_cursor: false,
            };
        }

        if let Some(plan) = self.try_reuse_live_at_cursor(true) {
            return plan;
        }

        if let Some(group) = self.hidden_group_candidate_at(self.cursor) {
            if self.storage.group_key_at(self.cursor) == Some(self.key)
                && (self.hidden_boundary_matches(Some(group.boundary_key))
                    || self.parent_boundary.policy().allows_live_search())
            {
                if let Some(matched_live_group) =
                    self.find_matching_live_group_after_hidden(group.scan_extent)
                {
                    return StartPlan::RestoreMatchingGroup {
                        matched_group: matched_live_group,
                        retire_conflicting_group_at_cursor: false,
                    };
                }
                return StartPlan::RestoreHiddenAtCursor {
                    scan_extent: group.scan_extent,
                    live_extent: group.live_extent,
                    boundary_key: group.boundary_key,
                };
            }
        }

        if let Some(matched_group) = self.find_matching_group() {
            return StartPlan::RestoreMatchingGroup {
                matched_group,
                retire_conflicting_group_at_cursor: self.has_conflicting_live_group_at_cursor(),
            };
        }

        StartPlan::InsertFresh {
            retire_conflicting_group_at_cursor: self.has_conflicting_live_group_at_cursor(),
        }
    }

    fn try_reuse_live_at_cursor(&self, allow_hidden_children: bool) -> Option<StartPlan> {
        if self.storage.entry_kind(self.cursor) != Some(EntryKind::live(EntryClass::Group)) {
            return None;
        }
        if !self.live_group_belongs_to_current_parent(self.cursor) {
            return None;
        }

        if self.storage.group_key_at(self.cursor) != Some(self.key)
            || !self.parent_boundary.policy().allows_exact_live_reuse()
        {
            return None;
        }
        let hidden_descendants = self.storage.group_hidden_descendants_at(self.cursor);
        if hidden_descendants > 0 && !allow_hidden_children {
            return None;
        }

        Some(StartPlan::ReuseLiveAtCursor {
            scan_extent: self.storage.group_scan_extent_at(self.cursor),
            live_extent: self.storage.group_live_extent_at(self.cursor),
            boundary_key: self.storage.group_retention_at(self.cursor)?.boundary_key(),
        })
    }

    fn previous_live_group_has_same_key(&self) -> bool {
        if self.cursor == 0
            || self.storage.entry_kind(self.cursor) != Some(EntryKind::live(EntryClass::Group))
            || !self.live_group_belongs_to_current_parent(self.cursor)
        {
            return false;
        }

        self.previous_live_sibling_root()
            .and_then(|index| self.storage.group_key_at(index))
            == Some(self.key)
    }

    fn has_conflicting_live_group_at_cursor(&self) -> bool {
        self.storage.entry_kind(self.cursor) == Some(EntryKind::live(EntryClass::Group))
            && self.live_group_belongs_to_current_parent(self.cursor)
            && self.storage.group_key_at(self.cursor) != Some(self.key)
    }

    fn hidden_group_candidate_at(&self, index: usize) -> Option<MatchedGroup> {
        if self.storage.entry_kind(index) != Some(EntryKind::hidden(EntryClass::Group)) {
            return None;
        }
        self.matched_group_at(index, true)
    }

    fn find_matching_live_group_after_hidden(
        &self,
        hidden_scan_extent: usize,
    ) -> Option<MatchedGroup> {
        let mut search_index = self.cursor.saturating_add(hidden_scan_extent.max(1));

        while search_index < self.parent_end {
            match self.storage.entry_kind(search_index) {
                Some(kind) if kind.matches(EntryClass::Group, EntryVisibility::Live) => {
                    if !self.live_group_belongs_to_current_parent(search_index) {
                        search_index = search_index
                            .saturating_add(self.storage.entry_scan_extent(search_index).max(1));
                        continue;
                    }
                    if self.storage.group_key_at(search_index) == Some(self.key) {
                        return self.matched_group_at(search_index, false);
                    }
                    search_index = search_index
                        .saturating_add(self.storage.entry_scan_extent(search_index).max(1));
                }
                Some(kind) if kind.matches(EntryClass::Group, EntryVisibility::Hidden) => {
                    search_index = search_index
                        .saturating_add(self.storage.entry_scan_extent(search_index).max(1));
                }
                Some(_) => search_index += 1,
                None => break,
            }
        }

        None
    }

    fn find_matching_group(&self) -> Option<MatchedGroup> {
        let mut search_index = self.cursor;

        while search_index < self.parent_end {
            match self.storage.entry_kind(search_index) {
                Some(kind) if kind.matches(EntryClass::Group, EntryVisibility::Live) => {
                    if !self.live_group_belongs_to_current_parent(search_index) {
                        search_index = search_index
                            .saturating_add(self.storage.entry_scan_extent(search_index).max(1));
                        continue;
                    }
                    if self.storage.group_key_at(search_index) == Some(self.key)
                        && self.parent_boundary.policy().allows_live_search()
                    {
                        return self.matched_group_at(search_index, false);
                    }
                    search_index = search_index
                        .saturating_add(self.storage.entry_scan_extent(search_index).max(1));
                }
                Some(kind) if kind.matches(EntryClass::Group, EntryVisibility::Hidden) => {
                    let boundary_key = self
                        .storage
                        .group_retention_at(search_index)?
                        .boundary_key();
                    if self.storage.group_key_at(search_index) == Some(self.key)
                        && (self.hidden_boundary_matches(Some(boundary_key))
                            || self.parent_boundary.policy().allows_live_search())
                    {
                        return self.matched_group_at(search_index, true);
                    }
                    search_index = search_index
                        .saturating_add(self.storage.entry_scan_extent(search_index).max(1));
                }
                Some(_) => search_index += 1,
                None => break,
            }
        }

        None
    }

    fn find_matching_hidden_group_in_previous_tail(&self) -> Option<MatchedGroup> {
        if self.cursor == 0
            || !self
                .storage
                .entry_kind(self.cursor - 1)
                .is_some_and(EntryKind::is_hidden)
        {
            return None;
        }

        let previous_sibling = self.previous_live_sibling_root()?;
        let previous_sibling_anchor = self.storage.entry_anchor(previous_sibling);
        let tail_start =
            previous_sibling + self.storage.group_live_extent_at(previous_sibling).max(1);
        let tail_end =
            previous_sibling + self.storage.group_scan_extent_at(previous_sibling).max(1);
        let mut index = tail_start;

        while index < tail_end {
            let Some(kind) = self.storage.entry_kind(index) else {
                break;
            };
            let extent = self.storage.entry_scan_extent(index).max(1);
            if kind.matches(EntryClass::Group, EntryVisibility::Hidden) {
                let stored_parent_anchor = self
                    .storage
                    .group_parent_anchor_at(index)
                    .unwrap_or(AnchorId::INVALID);
                let boundary_key = self.storage.group_retention_at(index)?.boundary_key();
                if (stored_parent_anchor == self.current_parent_anchor
                    || stored_parent_anchor == previous_sibling_anchor)
                    && self.storage.group_key_at(index) == Some(self.key)
                    && (self.hidden_boundary_matches(Some(boundary_key))
                        || self.parent_boundary.policy().allows_live_search())
                {
                    return self.matched_group_at(index, true);
                }
            }
            index += extent;
        }

        None
    }

    fn matched_group_at(&self, index: usize, reused_hidden: bool) -> Option<MatchedGroup> {
        Some(MatchedGroup {
            index,
            scan_extent: self.storage.group_scan_extent_at(index),
            live_extent: self.storage.group_live_extent_at(index),
            boundary_key: self.storage.group_retention_at(index)?.boundary_key(),
            reused_hidden,
        })
    }

    fn hidden_boundary_matches(&self, boundary_key: Option<Key>) -> bool {
        match (self.current_parent_boundary_key, boundary_key) {
            (Some(current), Some(boundary)) => current == boundary,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn previous_live_sibling_root(&self) -> Option<usize> {
        let anchor = self.previous_live_sibling_group?;
        let index = self.storage.resolve_anchor(anchor)?;
        let kind = self.storage.entry_kind(index)?;
        if !kind.matches(EntryClass::Group, EntryVisibility::Live)
            || !self.live_group_belongs_to_current_parent(index)
        {
            return None;
        }

        let group_end = index + self.storage.entry_scan_extent(index).max(1);
        (group_end == self.cursor).then_some(index)
    }

    fn live_group_belongs_to_current_parent(&self, index: usize) -> bool {
        self.storage
            .group_parent_anchor_at(index)
            .unwrap_or(AnchorId::INVALID)
            == self.current_parent_anchor
    }
}

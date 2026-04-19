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
        group: super::GroupSnapshot,
    },
    RestoreMatchingGroup {
        matched_group: MatchedGroup,
        retire_conflicting_group_at_cursor: bool,
    },
    InsertFresh {
        retire_conflicting_group_at_cursor: bool,
    },
}

pub(super) struct ReusePlanner<'a> {
    storage: &'a SlotStorage,
    key: Key,
    cursor: usize,
    parent_end: usize,
    parent_boundary: PassBoundary,
    current_parent_boundary_key: Option<Key>,
    current_parent_anchor: AnchorId,
}

impl<'a> ReusePlanner<'a> {
    pub(super) fn new(
        storage: &'a SlotStorage,
        key: Key,
        cursor: usize,
        parent_end: usize,
        parent_boundary: PassBoundary,
        current_parent_boundary_key: Option<Key>,
        current_parent_anchor: AnchorId,
    ) -> Self {
        Self {
            storage,
            key,
            cursor,
            parent_end,
            parent_boundary,
            current_parent_boundary_key,
            current_parent_anchor,
        }
    }

    pub(super) fn plan(&self) -> StartPlan {
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
            if group.key == self.key
                && (self.hidden_boundary_matches(Some(group.retention.boundary_key()))
                    || self.parent_boundary.allows_live_search())
            {
                if let Some(matched_live_group) = self.find_matching_live_group_after_hidden(&group)
                {
                    return StartPlan::RestoreMatchingGroup {
                        matched_group: matched_live_group,
                        retire_conflicting_group_at_cursor: false,
                    };
                }
                return StartPlan::RestoreHiddenAtCursor { group };
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

        let group = self.storage.group_snapshot_at(self.cursor)?;
        if group.key != self.key || !self.parent_boundary.policy().allows_exact_live_reuse() {
            return None;
        }
        if group.hidden_descendants > 0 && !allow_hidden_children {
            return None;
        }

        Some(StartPlan::ReuseLiveAtCursor {
            scan_extent: group.spans.scan_extent(),
            live_extent: group.spans.live_extent(),
            boundary_key: group.retention.boundary_key(),
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

    fn hidden_group_candidate_at(&self, index: usize) -> Option<super::GroupSnapshot> {
        (self.storage.entry_kind(index) == Some(EntryKind::hidden(EntryClass::Group)))
            .then(|| self.storage.group_snapshot_at(index))
            .flatten()
    }

    fn find_matching_live_group_after_hidden(
        &self,
        current_hidden_group: &super::GroupSnapshot,
    ) -> Option<MatchedGroup> {
        let mut search_index = self
            .cursor
            .saturating_add(current_hidden_group.spans.scan_extent().max(1));

        while search_index < self.parent_end {
            match self.storage.entry_kind(search_index) {
                Some(kind) if kind.matches(EntryClass::Group, EntryVisibility::Live) => {
                    if !self.live_group_belongs_to_current_parent(search_index) {
                        search_index = search_index
                            .saturating_add(self.storage.entry_scan_extent(search_index).max(1));
                        continue;
                    }
                    let group = self.storage.group_snapshot_at(search_index)?;
                    if group.key == self.key {
                        return Some(MatchedGroup {
                            index: search_index,
                            group,
                            reused_hidden: false,
                        });
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
                    let group = self.storage.group_snapshot_at(search_index)?;
                    if group.key == self.key && self.parent_boundary.policy().allows_live_search() {
                        return Some(MatchedGroup {
                            index: search_index,
                            group,
                            reused_hidden: false,
                        });
                    }
                    search_index = search_index
                        .saturating_add(self.storage.entry_scan_extent(search_index).max(1));
                }
                Some(kind) if kind.matches(EntryClass::Group, EntryVisibility::Hidden) => {
                    let group = self.storage.group_snapshot_at(search_index)?;
                    if group.key == self.key
                        && (self.hidden_boundary_matches(Some(group.retention.boundary_key()))
                            || self.parent_boundary.policy().allows_live_search())
                    {
                        return Some(MatchedGroup {
                            index: search_index,
                            group,
                            reused_hidden: true,
                        });
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

    fn hidden_boundary_matches(&self, boundary_key: Option<Key>) -> bool {
        match (self.current_parent_boundary_key, boundary_key) {
            (Some(current), Some(boundary)) => current == boundary,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn previous_live_sibling_root(&self) -> Option<usize> {
        let mut search_index = self.cursor;
        while search_index > 0 {
            search_index -= 1;
            let kind = self.storage.entry_kind(search_index)?;
            if !kind.matches(EntryClass::Group, EntryVisibility::Live) {
                continue;
            }
            if !self.live_group_belongs_to_current_parent(search_index) {
                continue;
            }
            let group_end = search_index + self.storage.entry_scan_extent(search_index).max(1);
            if group_end == self.cursor {
                return Some(search_index);
            }
        }
        None
    }

    fn live_group_belongs_to_current_parent(&self, index: usize) -> bool {
        self.storage
            .group_parent_anchor_at(index)
            .unwrap_or(AnchorId::INVALID)
            == self.current_parent_anchor
    }
}

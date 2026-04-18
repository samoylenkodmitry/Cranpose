use crate::Key;

use super::{
    storage::{EntryKind, SlotStorage},
    MatchedGroup, ReuseState,
};

pub(super) enum StartPlan {
    ReuseLiveAtCursor {
        len: usize,
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
    parent_reuse: ReuseState,
    current_parent_boundary_key: Option<Key>,
}

impl<'a> ReusePlanner<'a> {
    pub(super) fn new(
        storage: &'a SlotStorage,
        key: Key,
        cursor: usize,
        parent_end: usize,
        parent_reuse: ReuseState,
        current_parent_boundary_key: Option<Key>,
    ) -> Self {
        Self {
            storage,
            key,
            cursor,
            parent_end,
            parent_reuse,
            current_parent_boundary_key,
        }
    }

    pub(super) fn plan(&self) -> StartPlan {
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
                && (self.hidden_boundary_matches(Some(group.boundary_key))
                    || self.parent_reuse.allows_live_search())
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
        if self.storage.entry_kind(self.cursor) != Some(EntryKind::Group) {
            return None;
        }

        let group = self.storage.group_snapshot_at(self.cursor)?;
        if group.key != self.key || !self.parent_reuse.allows_exact_live_reuse() {
            return None;
        }
        if group.has_hidden_children && !allow_hidden_children {
            return None;
        }

        Some(StartPlan::ReuseLiveAtCursor {
            len: group.len as usize,
            boundary_key: group.boundary_key,
        })
    }

    fn previous_live_group_has_same_key(&self) -> bool {
        if self.cursor == 0 || self.storage.entry_kind(self.cursor) != Some(EntryKind::Group) {
            return false;
        }

        matches!(
            self.storage.entry_kind(self.cursor - 1),
            Some(EntryKind::Group)
        ) && self.storage.group_key_at(self.cursor - 1) == Some(self.key)
    }

    fn has_conflicting_live_group_at_cursor(&self) -> bool {
        self.storage.entry_kind(self.cursor) == Some(EntryKind::Group)
            && self.storage.group_key_at(self.cursor) != Some(self.key)
    }

    fn hidden_group_candidate_at(&self, index: usize) -> Option<super::GroupSnapshot> {
        (self.storage.entry_kind(index) == Some(EntryKind::HiddenGroup))
            .then(|| self.storage.group_snapshot_at(index))
            .flatten()
    }

    fn find_matching_live_group_after_hidden(
        &self,
        current_hidden_group: &super::GroupSnapshot,
    ) -> Option<MatchedGroup> {
        let mut search_index = self
            .cursor
            .saturating_add((current_hidden_group.len as usize).max(1));

        while search_index < self.parent_end {
            match self.storage.entry_kind(search_index) {
                Some(EntryKind::Group) => {
                    let group = self.storage.group_snapshot_at(search_index)?;
                    if group.key == self.key {
                        return Some(MatchedGroup {
                            index: search_index,
                            group,
                            reused_hidden: false,
                        });
                    }
                    search_index =
                        search_index.saturating_add(self.storage.entry_extent(search_index).max(1));
                }
                Some(EntryKind::HiddenGroup) => {
                    search_index =
                        search_index.saturating_add(self.storage.entry_extent(search_index).max(1));
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
                Some(EntryKind::Group) => {
                    let group = self.storage.group_snapshot_at(search_index)?;
                    if group.key == self.key && self.parent_reuse.allows_live_search() {
                        return Some(MatchedGroup {
                            index: search_index,
                            group,
                            reused_hidden: false,
                        });
                    }
                    search_index =
                        search_index.saturating_add(self.storage.entry_extent(search_index).max(1));
                }
                Some(EntryKind::HiddenGroup) => {
                    let group = self.storage.group_snapshot_at(search_index)?;
                    if group.key == self.key
                        && (self.hidden_boundary_matches(Some(group.boundary_key))
                            || self.parent_reuse.allows_live_search())
                    {
                        return Some(MatchedGroup {
                            index: search_index,
                            group,
                            reused_hidden: true,
                        });
                    }
                    search_index =
                        search_index.saturating_add(self.storage.entry_extent(search_index).max(1));
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
}

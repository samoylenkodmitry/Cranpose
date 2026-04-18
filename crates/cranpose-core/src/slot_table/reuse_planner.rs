use crate::{AnchorId, Key};

use super::{unpack_slot_len, ChildReusePolicy, MatchedGroup, PreservedGroup, Slot};

pub(super) enum StartPlan {
    ReuseLiveAtCursor {
        len: usize,
        gap_boundary_key: Key,
    },
    RestoreGapAtCursor {
        anchor: AnchorId,
        group: PreservedGroup,
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
    slots: &'a [Slot],
    key: Key,
    cursor: usize,
    parent_end: usize,
    allow_exact_live_reuse: bool,
    allow_live_search: bool,
    current_parent_gap_boundary_key: Option<Key>,
}

impl<'a> ReusePlanner<'a> {
    pub(super) fn new(
        slots: &'a [Slot],
        key: Key,
        cursor: usize,
        parent_end: usize,
        parent_reuse: ChildReusePolicy,
        current_parent_gap_boundary_key: Option<Key>,
    ) -> Self {
        Self {
            slots,
            key,
            cursor,
            parent_end,
            allow_exact_live_reuse: parent_reuse.allows_exact_live_reuse(),
            allow_live_search: matches!(parent_reuse, ChildReusePolicy::Normal),
            current_parent_gap_boundary_key,
        }
    }

    pub(super) fn plan(&self) -> StartPlan {
        if let Some(plan) = self.try_reuse_live_at_cursor(false) {
            return plan;
        }

        if self.previous_group_has_same_key() {
            return StartPlan::InsertFresh {
                retire_conflicting_group_at_cursor: false,
            };
        }

        if let Some(plan) = self.try_reuse_live_at_cursor(true) {
            return plan;
        }

        if let Some((anchor, group)) = self.gap_group_candidate_at(self.cursor) {
            if group.key == self.key
                && (self.gap_matches_current_boundary(Some(group.boundary_key))
                    || self.allow_live_search)
            {
                if let Some(matched_live_group) = self.find_matching_live_group_after_gap(&group) {
                    return StartPlan::RestoreMatchingGroup {
                        matched_group: matched_live_group,
                        retire_conflicting_group_at_cursor: false,
                    };
                }
                return StartPlan::RestoreGapAtCursor { anchor, group };
            }
        }

        if let Some(matched_group) = self.find_matching_group() {
            return StartPlan::RestoreMatchingGroup {
                matched_group,
                retire_conflicting_group_at_cursor: self.has_conflicting_group_at_cursor(),
            };
        }

        StartPlan::InsertFresh {
            retire_conflicting_group_at_cursor: self.has_conflicting_group_at_cursor(),
        }
    }

    fn try_reuse_live_at_cursor(&self, allow_gap_children: bool) -> Option<StartPlan> {
        let Some(Slot::Group {
            key,
            len,
            boundary_key,
            has_gap_children,
            ..
        }) = self.slots.get(self.cursor)
        else {
            return None;
        };

        if *key != self.key || !self.allow_exact_live_reuse {
            return None;
        }
        if *has_gap_children && !allow_gap_children {
            return None;
        }

        Some(StartPlan::ReuseLiveAtCursor {
            len: unpack_slot_len(*len),
            gap_boundary_key: *boundary_key,
        })
    }

    fn previous_group_has_same_key(&self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        if matches!(self.slots.get(self.cursor), Some(Slot::Gap { .. })) {
            return false;
        }

        matches!(
            self.slots.get(self.cursor - 1),
            Some(Slot::Group { key, .. }) if *key == self.key
        )
    }

    fn has_conflicting_group_at_cursor(&self) -> bool {
        matches!(
            self.slots.get(self.cursor),
            Some(Slot::Group { key, .. }) if *key != self.key
        )
    }

    fn gap_group_candidate_at(&self, index: usize) -> Option<(AnchorId, PreservedGroup)> {
        let Some(Slot::Gap { anchor, metadata }) = self.slots.get(index) else {
            return None;
        };

        Some((*anchor, metadata.preserved_group.clone()?))
    }

    fn find_matching_live_group_after_gap(
        &self,
        current_gap_group: &PreservedGroup,
    ) -> Option<MatchedGroup> {
        let mut search_index = self
            .cursor
            .saturating_add(unpack_slot_len(current_gap_group.len).max(1));

        while search_index < self.parent_end {
            match self.slots.get(search_index) {
                Some(Slot::Group {
                    key,
                    anchor,
                    len,
                    boundary_key,
                    scope,
                    ..
                }) if *key == self.key => {
                    return Some(MatchedGroup {
                        index: search_index,
                        anchor: *anchor,
                        group: PreservedGroup {
                            key: *key,
                            len: *len,
                            boundary_key: *boundary_key,
                            scope: scope.clone(),
                        },
                        gap_boundary_key: *boundary_key,
                        reused_gap: false,
                    });
                }
                Some(Slot::Group { len, .. }) => {
                    search_index = search_index.saturating_add(unpack_slot_len(*len).max(1));
                }
                Some(Slot::Gap { .. }) => {
                    search_index = search_index.saturating_add(self.slot_extent(search_index));
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
            match self.slots.get(search_index) {
                Some(Slot::Group {
                    key,
                    anchor,
                    len,
                    boundary_key,
                    scope,
                    ..
                }) => {
                    if *key == self.key && self.allow_live_search {
                        return Some(MatchedGroup {
                            index: search_index,
                            anchor: *anchor,
                            group: PreservedGroup {
                                key: *key,
                                len: *len,
                                boundary_key: *boundary_key,
                                scope: scope.clone(),
                            },
                            gap_boundary_key: *boundary_key,
                            reused_gap: false,
                        });
                    }
                    search_index = search_index.saturating_add(unpack_slot_len(*len).max(1));
                }
                Some(Slot::Gap { anchor, metadata }) => {
                    let Some(group) = metadata.preserved_group.clone() else {
                        search_index = search_index.saturating_add(self.slot_extent(search_index));
                        continue;
                    };
                    if group.key == self.key
                        && (self.gap_matches_current_boundary(Some(group.boundary_key))
                            || self.allow_live_search)
                    {
                        return Some(MatchedGroup {
                            index: search_index,
                            anchor: *anchor,
                            group: group.clone(),
                            gap_boundary_key: group.boundary_key,
                            reused_gap: true,
                        });
                    }
                    search_index = search_index.saturating_add(self.slot_extent(search_index));
                }
                Some(_) => search_index += 1,
                None => break,
            }
        }

        None
    }

    fn gap_matches_current_boundary(&self, boundary_key: Option<Key>) -> bool {
        match (self.current_parent_gap_boundary_key, boundary_key) {
            (Some(current), Some(boundary)) => current == boundary,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn slot_extent(&self, index: usize) -> usize {
        match self.slots.get(index) {
            Some(Slot::Group { len, .. }) => unpack_slot_len(*len).max(1),
            Some(Slot::Gap { metadata, .. }) => unpack_slot_len(metadata.extent).max(1),
            Some(_) => 1,
            None => 0,
        }
    }
}

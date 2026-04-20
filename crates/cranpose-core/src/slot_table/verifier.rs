#[cfg(debug_assertions)]
use super::{
    lifecycle_queue::SlotLifecycleCoordinator,
    storage::{EntryClass, EntryKind, EntryVisibility},
    SlotTable,
};
#[cfg(not(debug_assertions))]
use super::{lifecycle_queue::SlotLifecycleCoordinator, SlotTable};
#[cfg(debug_assertions)]
use crate::AnchorId;
#[cfg(debug_assertions)]
use std::collections::HashSet;

#[cfg(debug_assertions)]
pub(crate) struct SlotTableVerifier<'a> {
    table: &'a SlotTable,
    lifecycle: Option<&'a SlotLifecycleCoordinator>,
    seen_anchors: HashSet<usize>,
}

#[cfg(debug_assertions)]
impl<'a> SlotTableVerifier<'a> {
    pub(crate) fn new(
        table: &'a SlotTable,
        lifecycle: Option<&'a SlotLifecycleCoordinator>,
    ) -> Self {
        Self {
            table,
            lifecycle,
            seen_anchors: HashSet::new(),
        }
    }

    pub(crate) fn verify(mut self) -> Result<(), String> {
        let len = self.table.storage.len();
        let (visited_end, _) = self.verify_range(AnchorId::INVALID, 0, len)?;
        if visited_end != len {
            return Err(format!(
                "slot table traversal ended at {visited_end}, expected {len}"
            ));
        }
        if let Some(lifecycle) = self.lifecycle {
            let orphaned = lifecycle.orphaned_node_ids_snapshot();
            if !orphaned.is_empty() {
                return Err(format!(
                    "orphaned node queue must be empty after pass finalization, found {} entries",
                    orphaned.len()
                ));
            }
            let preserved_orphaned = lifecycle.preserved_orphaned_node_ids_snapshot();
            for orphaned in preserved_orphaned {
                if self.table.orphaned_node_state(orphaned) != super::NodeSlotState::PreservedGap {
                    return Err(format!(
                        "preserved orphan {:?} no longer resolves to a preserved gap entry",
                        orphaned
                    ));
                }
            }
        }
        Ok(())
    }

    fn verify_range(
        &mut self,
        expected_parent: AnchorId,
        start: usize,
        end: usize,
    ) -> Result<(usize, usize), String> {
        let mut index = start;
        let table_len = self.table.storage.len();
        let mut hidden_entries_in_range = 0usize;

        while index < end {
            let Some(entry) = self.table.storage.entry(index) else {
                return Err(format!("missing entry at slot index {index}"));
            };

            match entry.kind {
                EntryKind::Unused => {
                    index += 1;
                }
                EntryKind::Occupied {
                    class: EntryClass::Group,
                    visibility,
                } => {
                    self.verify_anchor(index, entry.anchor)?;
                    let Some(group) = self.table.storage.group_snapshot_at(index) else {
                        return Err(format!("missing group snapshot at slot index {index}"));
                    };
                    let live_extent = group.spans.live_extent();
                    let scan_extent = group.spans.scan_extent();
                    if live_extent == 0 {
                        return Err(format!("group at slot {index} has zero live extent"));
                    }
                    if scan_extent == 0 {
                        return Err(format!("group at slot {index} has zero scan extent"));
                    }
                    if scan_extent < live_extent {
                        return Err(format!(
                            "group at slot {index} has invalid spans: live={live_extent} scan={scan_extent}"
                        ));
                    }
                    let group_end = index + scan_extent;
                    if group_end > table_len {
                        return Err(format!(
                            "group at slot {index} extends past table end: group_end={group_end} table_len={table_len}"
                        ));
                    }

                    let parent_anchor = self
                        .table
                        .storage
                        .group_parent_anchor_at(index)
                        .unwrap_or(AnchorId::INVALID);
                    if expected_parent.is_valid() {
                        if parent_anchor != expected_parent {
                            let actual_parent_slot = parent_anchor
                                .is_valid()
                                .then(|| self.table.storage.resolve_anchor(parent_anchor))
                                .flatten();
                            let actual_parent_anchor_owner =
                                (0..self.table.storage.len()).find(|slot| {
                                    self.table.storage.entry_anchor(*slot) == parent_anchor
                                });
                            let expected_parent_slot =
                                self.table.storage.resolve_anchor(expected_parent);
                            return Err(format!(
                                "group at slot {index} has wrong parent anchor {:?} resolving to {:?} owned_by_slot={:?}, expected {:?} resolving to {:?}",
                                parent_anchor,
                                actual_parent_slot,
                                actual_parent_anchor_owner,
                                expected_parent,
                                expected_parent_slot
                            ));
                        }
                    } else if parent_anchor.is_valid() {
                        return Err(format!(
                            "group at slot {index} unexpectedly has parent anchor {:?} at the root",
                            parent_anchor
                        ));
                    }

                    if parent_anchor.is_valid() {
                        let Some(parent_index) = self.table.storage.resolve_anchor(parent_anchor)
                        else {
                            return Err(format!(
                                "group at slot {index} has unresolved parent anchor {:?}",
                                parent_anchor
                            ));
                        };
                        if parent_index >= index {
                            return Err(format!(
                                "group at slot {index} has parent anchor {:?} resolving forward to slot {parent_index}",
                                parent_anchor
                            ));
                        }
                    }

                    let (next_index, hidden_descendants) =
                        self.verify_range(entry.anchor, index + 1, group_end)?;
                    if next_index != group_end {
                        return Err(format!(
                            "group at slot {index} traversal ended at {next_index}, expected {group_end}"
                        ));
                    }
                    let stored_hidden_descendants =
                        self.table.storage.group_hidden_descendants_at(index);
                    if stored_hidden_descendants != hidden_descendants {
                        return Err(format!(
                            "group at slot {index} has hidden-descendant drift: stored={} actual={hidden_descendants}",
                            stored_hidden_descendants,
                        ));
                    }
                    if visibility == EntryVisibility::Hidden
                        && hidden_descendants.saturating_add(1) != scan_extent
                    {
                        return Err(format!(
                            "hidden group at slot {index} contains non-hidden descendants: hidden_descendants={hidden_descendants} scan_extent={scan_extent}"
                        ));
                    }

                    if let Some(scope) = group.scope.as_ref() {
                        if scope.group_anchor() != entry.anchor {
                            return Err(format!(
                                "scope anchor mismatch at group {index}: scope={:?} entry={:?}",
                                scope.group_anchor(),
                                entry.anchor,
                            ));
                        }
                        if self.table.storage.resolve_anchor(scope.group_anchor()) != Some(index) {
                            return Err(format!(
                                "scope anchor for group {index} does not resolve back to the group"
                            ));
                        }
                    }

                    hidden_entries_in_range += hidden_descendants;
                    if visibility == EntryVisibility::Hidden {
                        hidden_entries_in_range += 1;
                    }
                    index = group_end;
                }
                EntryKind::Occupied {
                    class: EntryClass::Value,
                    visibility,
                } => {
                    self.verify_missing_anchor(index, entry.anchor, "value")?;
                    if visibility == EntryVisibility::Hidden {
                        hidden_entries_in_range += 1;
                    }
                    index += 1;
                }
                EntryKind::Occupied {
                    class: EntryClass::Node,
                    visibility,
                } => {
                    self.verify_anchor(index, entry.anchor)?;
                    if visibility == EntryVisibility::Hidden {
                        hidden_entries_in_range += 1;
                    }
                    index += 1;
                }
            }
        }

        Ok((index, hidden_entries_in_range))
    }

    fn verify_anchor(&mut self, index: usize, anchor: AnchorId) -> Result<(), String> {
        if !anchor.is_valid() {
            return Err(format!("occupied entry at slot {index} has invalid anchor"));
        }
        if !self.seen_anchors.insert(anchor.0) {
            return Err(format!("duplicate live anchor {:?} detected", anchor));
        }
        if self.table.storage.resolve_anchor(anchor) != Some(index) {
            return Err(format!(
                "anchor {:?} does not resolve to slot {index}",
                anchor
            ));
        }
        Ok(())
    }

    fn verify_missing_anchor(
        &mut self,
        index: usize,
        anchor: AnchorId,
        kind: &str,
    ) -> Result<(), String> {
        if anchor.is_valid() {
            return Err(format!(
                "{kind} at slot {index} unexpectedly has anchor {anchor:?}"
            ));
        }
        Ok(())
    }
}

#[cfg(debug_assertions)]
pub(crate) fn debug_verify_slot_table(
    table: &SlotTable,
    lifecycle: Option<&SlotLifecycleCoordinator>,
) {
    if let Err(error) = SlotTableVerifier::new(table, lifecycle).verify() {
        panic!(
            "slot table verification failed: {error}\nslots={:?}\ngroups={:?}",
            table.debug_dump_all_slots(),
            table.debug_dump_groups(),
        );
    }
}

#[cfg(not(debug_assertions))]
pub(crate) fn debug_verify_slot_table(
    _table: &SlotTable,
    _lifecycle: Option<&SlotLifecycleCoordinator>,
) {
}

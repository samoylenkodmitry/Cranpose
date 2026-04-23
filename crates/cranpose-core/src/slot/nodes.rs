use super::{GroupRecord, NodeLifecycle, NodeRecord, SlotTable};
use crate::{collections::map::HashSet, NodeId};
use std::{mem, ops::Range};

pub(super) struct GroupNodeRecordResult {
    pub(super) reused_slot: bool,
    pub(super) reused_node: bool,
}

impl SlotTable {
    fn group_node_start_at(&self, group_index: usize) -> usize {
        self.groups[group_index].node_start as usize
    }

    pub(in crate::slot) fn group_node_len_at(&self, group_index: usize) -> usize {
        self.groups[group_index].node_len as usize
    }

    pub(in crate::slot) fn group_node_range_checked(
        &self,
        group_index: usize,
    ) -> Option<Range<usize>> {
        let start = self.group_node_start_at(group_index);
        let len = self.group_node_len_at(group_index);
        let end = start.checked_add(len)?;
        (end <= self.nodes.len()).then_some(start..end)
    }

    fn group_node_range_at(&self, group_index: usize) -> Range<usize> {
        self.group_node_range_checked(group_index)
            .expect("node range should resolve")
    }

    fn apply_node_start_delta(node_start: &mut u32, delta: i64) {
        let updated = (*node_start as i64) + delta;
        debug_assert!(updated >= 0, "node start cannot become negative");
        *node_start = updated as u32;
    }

    fn shift_node_starts_from(&mut self, start_group_index: usize, delta: i64) {
        if delta == 0 {
            return;
        }
        for group in &mut self.groups[start_group_index..] {
            Self::apply_node_start_delta(&mut group.node_start, delta);
        }
    }

    fn offset_detached_group_node_starts(groups: &mut [GroupRecord], delta: i64) {
        if delta == 0 {
            return;
        }
        for group in groups {
            Self::apply_node_start_delta(&mut group.node_start, delta);
        }
    }

    fn subtree_node_span(groups: &[GroupRecord]) -> Option<(usize, usize)> {
        let node_start = groups.first()?.node_start as usize;
        let node_len = groups
            .iter()
            .map(|group| group.node_len as usize)
            .sum::<usize>();
        Some((node_start, node_len))
    }

    pub(in crate::slot) fn group_node_records_at(&self, group_index: usize) -> &[NodeRecord] {
        let range = self.group_node_range_at(group_index);
        &self.nodes[range]
    }

    pub(in crate::slot) fn group_node_record_at(
        &self,
        group_index: usize,
        node_index: usize,
    ) -> &NodeRecord {
        self.group_node_records_at(group_index)
            .get(node_index)
            .expect("node index should resolve")
    }

    pub(in crate::slot) fn group_node_record_at_mut(
        &mut self,
        group_index: usize,
        node_index: usize,
    ) -> &mut NodeRecord {
        let node_start = self.group_node_start_at(group_index);
        self.nodes
            .get_mut(node_start + node_index)
            .expect("node index should resolve")
    }

    pub(in crate::slot) fn total_node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(super) fn node_heap_bytes(&self) -> usize {
        self.nodes.capacity() * mem::size_of::<NodeRecord>()
    }

    pub(super) fn node_debug_capacity(&self) -> usize {
        self.nodes.capacity()
    }

    pub(super) fn record_group_node(
        &mut self,
        group_index: usize,
        node_index: usize,
        owner: crate::AnchorId,
        id: NodeId,
        parent_id: Option<NodeId>,
        generation: u32,
    ) -> GroupNodeRecordResult {
        if node_index < self.group_node_len_at(group_index) {
            let existing = *self.group_node_record_at(group_index, node_index);
            *self.group_node_record_at_mut(group_index, node_index) = NodeRecord {
                owner,
                id,
                parent_id,
                generation,
                lifecycle: NodeLifecycle::Active,
            };
            GroupNodeRecordResult {
                reused_slot: true,
                reused_node: existing.id == id && existing.generation == generation,
            }
        } else {
            let insert_index = self.group_node_start_at(group_index) + node_index;
            self.nodes.insert(
                insert_index,
                NodeRecord {
                    owner,
                    id,
                    parent_id,
                    generation,
                    lifecycle: NodeLifecycle::Active,
                },
            );
            self.groups[group_index].node_len += 1;
            self.shift_node_starts_from(group_index + 1, 1);
            GroupNodeRecordResult {
                reused_slot: false,
                reused_node: false,
            }
        }
    }

    pub(super) fn remove_group_node_range(
        &mut self,
        group_index: usize,
        start: usize,
    ) -> Vec<NodeRecord> {
        let node_len = self.group_node_len_at(group_index);
        if start >= node_len {
            return Vec::new();
        }
        let node_start = self.group_node_start_at(group_index) + start;
        let node_end = self.group_node_start_at(group_index) + node_len;
        let removed = self.nodes.drain(node_start..node_end).collect::<Vec<_>>();
        self.groups[group_index].node_len -= removed.len() as u32;
        self.shift_node_starts_from(group_index + 1, -(removed.len() as i64));
        removed
    }

    pub(super) fn detach_nodes_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
    ) -> Vec<NodeRecord> {
        let Some((node_start, node_len)) = Self::subtree_node_span(removed_groups) else {
            return Vec::new();
        };
        Self::offset_detached_group_node_starts(removed_groups, -(node_start as i64));
        if node_len == 0 {
            return Vec::new();
        }
        let removed = self
            .nodes
            .drain(node_start..node_start + node_len)
            .collect();
        self.shift_node_starts_from(removed_group_index, -(node_len as i64));
        removed
    }

    pub(super) fn restore_nodes_for_groups(
        &mut self,
        insert_group_index: usize,
        groups: &mut [GroupRecord],
        nodes: Vec<NodeRecord>,
    ) {
        let node_insert_index = if insert_group_index < self.groups.len() {
            self.groups[insert_group_index].node_start as usize
        } else {
            self.nodes.len()
        };
        self.shift_node_starts_from(insert_group_index, nodes.len() as i64);
        Self::offset_detached_group_node_starts(groups, node_insert_index as i64);
        self.nodes
            .splice(node_insert_index..node_insert_index, nodes);
    }

    pub(in crate::slot) fn root_node_ids_from_records(nodes: &[NodeRecord]) -> Vec<NodeId> {
        let node_set = nodes.iter().map(|node| node.id).collect::<HashSet<_>>();
        nodes
            .iter()
            .filter(|node| {
                node.parent_id
                    .is_none_or(|parent_id| !node_set.contains(&parent_id))
            })
            .map(|node| node.id)
            .collect()
    }
}

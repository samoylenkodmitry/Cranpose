use std::ops::ControlFlow;

use super::super::{
    GroupRecord, NodeRecord, NodeSlotUpdate, RootNodeIds, SlotTable, SlotWriteSession,
};
use crate::NodeId;

impl SlotTable {
    pub(in crate::slot) fn subtree_root_node_ids_at(&self, group_index: usize) -> RootNodeIds {
        let mut roots = RootNodeIds::new();
        self.for_each_subtree_root_node_id_at(group_index, |id| roots.push(id));
        roots
    }

    /// Calls `f` with each root node of the subtree at `group_index`, the
    /// records sharing its first record's parent, in the order the
    /// composition emitted them.
    pub(in crate::slot) fn for_each_subtree_root_node_id_at(
        &self,
        group_index: usize,
        mut f: impl FnMut(NodeId),
    ) {
        let Some(parent) = self
            .subtree_first_record_at(group_index)
            .map(|first| first.parent_id)
        else {
            return;
        };
        #[cfg(any(test, debug_assertions))]
        self.debug_assert_roots_in_emission_order(group_index, parent);
        let _ = self.try_for_each_root_in(group_index, parent, &mut |id| {
            f(id);
            ControlFlow::<()>::Continue(())
        });
    }

    /// The root node the subtree at `group_index` emitted first.
    pub(in crate::slot) fn first_subtree_root_node_id_at(
        &self,
        group_index: usize,
    ) -> Option<NodeId> {
        let first = self.subtree_first_record_at(group_index)?;
        // The subtree's first record is a root. The groups above its own
        // group hold no records, so it is the first one emitted unless its
        // group composed a child group before it.
        let found = if first.child_groups_before == 0 {
            Some(first.id)
        } else {
            self.try_for_each_root_in(group_index, first.parent_id, &mut ControlFlow::Break)
                .break_value()
        };
        #[cfg(any(test, debug_assertions))]
        if crate::slot::slot_validation_diagnostics_enabled() {
            assert_eq!(
                found,
                self.try_for_each_root_in(group_index, first.parent_id, &mut ControlFlow::Break)
                    .break_value(),
                "the first root must be the first one the subtree emitted"
            );
        }
        found
    }

    /// The subtree's first record, whose parent all its roots share. `None`
    /// when the subtree holds no nodes.
    fn subtree_first_record_at(&self, group_index: usize) -> Option<&NodeRecord> {
        let group = self.groups.get(group_index)?;
        if group.subtree_node_count == 0 {
            return None;
        }
        let first = self.nodes.get(group.node_start as usize);
        if first.is_none() {
            log::error!(
                "slot table ignored root-node collection past node storage at group index {group_index}"
            );
        }
        first
    }

    pub(in crate::slot) fn own_node_records(&self, group: &GroupRecord) -> &[NodeRecord] {
        let start = group.node_start as usize;
        start
            .checked_add(group.node_len as usize)
            .and_then(|end| self.nodes.get(start..end))
            .unwrap_or_default()
    }

    /// Visits the records of the subtree at `group_index` whose parent is
    /// `parent`: the group's own records, each placed among its child groups
    /// by the children composed before it, and the roots of each child whose
    /// first record has that parent. A child whose first record has another
    /// parent sits inside a node of this subtree, and so do all its roots.
    pub(in crate::slot) fn try_for_each_root_in<B>(
        &self,
        group_index: usize,
        parent: Option<NodeId>,
        f: &mut impl FnMut(NodeId) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        let Some(group) = self.groups.get(group_index) else {
            return ControlFlow::Continue(());
        };
        let own = self.own_node_records(group);
        let mut child_nodes = group.subtree_node_count.saturating_sub(group.node_len);
        let mut next_own = 0;
        if child_nodes > 0 {
            let end = group_index.saturating_add(group.subtree_len as usize);
            let mut child_index = group_index + 1;
            let mut children_before = 0;
            while child_nodes > 0 && child_index < end {
                while let Some(record) = own
                    .get(next_own)
                    .filter(|record| record.child_groups_before <= children_before)
                {
                    if record.parent_id == parent {
                        f(record.id)?;
                    }
                    next_own += 1;
                }
                let Some(child) = self.groups.get(child_index) else {
                    log::error!(
                        "slot table stopped root-node collection at missing child group index {child_index}"
                    );
                    break;
                };
                if child.subtree_node_count > 0 {
                    let first_parent = self
                        .nodes
                        .get(child.node_start as usize)
                        .map(|record| record.parent_id);
                    if first_parent == Some(parent) {
                        self.try_for_each_root_in(child_index, parent, f)?;
                    }
                    child_nodes = child_nodes.saturating_sub(child.subtree_node_count);
                }
                children_before += 1;
                child_index += (child.subtree_len as usize).max(1);
            }
        }
        for record in &own[next_own..] {
            if record.parent_id == parent {
                f(record.id)?;
            }
        }
        ControlFlow::Continue(())
    }

    #[cfg(any(test, debug_assertions))]
    fn debug_assert_roots_in_emission_order(&self, group_index: usize, parent: Option<NodeId>) {
        if !crate::slot::slot_validation_diagnostics_enabled() {
            return;
        }
        let Some(group) = self.groups.get(group_index) else {
            return;
        };
        let start = group.node_start as usize;
        let records = start
            .checked_add(group.subtree_node_count as usize)
            .and_then(|end| self.nodes.get(start..end))
            .unwrap_or_default();
        let mut emitted = Vec::new();
        let _ = self.try_for_each_root_in(group_index, parent, &mut |id| {
            emitted.push(id);
            ControlFlow::<()>::Continue(())
        });
        let mut stored: Vec<_> = super::super::types::root_node_ids(records).collect();
        emitted.sort_unstable();
        stored.sort_unstable();
        assert_eq!(
            emitted, stored,
            "the roots found in emission order must be the subtree's root records"
        );
    }
}

/// A node record of the open group that a node emitted again may adopt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FoundNodeRecord {
    index: usize,
    pub(crate) id: NodeId,
    pub(crate) generation: u32,
}

impl SlotWriteSession<'_> {
    fn open_node_group(&self) -> Option<(usize, usize)> {
        let frame = self.state.group_stack.last()?;
        let group_index = self
            .table
            .open_group_index(frame.group_anchor, frame.group_index)?;
        Some((group_index, frame.node_cursor))
    }

    pub(crate) fn record_node_with_parent(
        &mut self,
        id: NodeId,
        generation: u32,
        parent_id: Option<NodeId>,
        source: crate::Key,
    ) -> NodeSlotUpdate {
        let source = self.state.mix_branch_fold(source);
        let Some(frame) = self.state.group_stack.last() else {
            log::error!(
                "slot writer record_node_with_parent called with an empty group stack; id={id}"
            );
            return NodeSlotUpdate::Inserted { id, generation };
        };
        let (group_anchor, node_cursor) = (frame.group_anchor, frame.node_cursor);
        let result = match self.table.open_group_index(group_anchor, frame.group_index) {
            Some(group_index) => self.table.record_node_at(
                group_index,
                node_cursor,
                NodeRecord::active(
                    group_anchor,
                    id,
                    parent_id,
                    generation,
                    source,
                    frame.ended_children,
                ),
            ),
            None => {
                log::error!(
                    "slot table ignored node record for stale owner anchor {group_anchor:?}; node id={id}"
                );
                NodeSlotUpdate::Inserted { id, generation }
            }
        };
        if let Some(frame) = self.state.group_stack.last_mut() {
            frame.advance_node_cursor();
        }
        result
    }

    /// The `skip_matches`th node record at or after the node cursor that a
    /// node emitted from `source` left.
    pub(crate) fn peek_node_record_by_source(
        &mut self,
        source: crate::Key,
        skip_matches: usize,
    ) -> Option<FoundNodeRecord> {
        let mixed = self.state.mix_branch_fold(source);
        let (group_index, cursor) = self.open_node_group()?;
        let mut from = cursor;
        let mut remaining = skip_matches;
        loop {
            let (index, id, generation) =
                self.table
                    .find_node_record_by_source_at(group_index, from, mixed)?;
            if remaining == 0 {
                return Some(FoundNodeRecord {
                    index,
                    id,
                    generation,
                });
            }
            remaining -= 1;
            from = index + 1;
        }
    }

    /// Moves `found` to the node cursor and records its node there.
    pub(crate) fn adopt_node_record(
        &mut self,
        found: FoundNodeRecord,
        parent_id: Option<NodeId>,
        source: crate::Key,
    ) -> NodeSlotUpdate {
        if let Some((group_index, cursor)) = self.open_node_group()
            && found.index > cursor
        {
            self.table
                .rotate_node_record_to_cursor_at(group_index, found.index, cursor);
        }
        self.record_node_with_parent(found.id, found.generation, parent_id, source)
    }

    #[cfg(test)]
    pub(crate) fn current_node_record(&mut self) -> Option<(NodeId, u32, crate::Key)> {
        let frame = self.state.group_stack.last()?;
        self.table
            .node_identity_at_cursor(frame.group_anchor, frame.node_cursor)
    }
}

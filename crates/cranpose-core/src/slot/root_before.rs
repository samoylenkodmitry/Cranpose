//! Where the first root node of a group that holds none goes among its
//! parent node's children: after the root the composition emitted last
//! before the group in that parent.

use std::{convert::Infallible, ops::ControlFlow};

use super::SlotTable;
use crate::NodeId;

// What the composition emitted in a parent node before a group.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RootBefore {
    // This root, the last one before the group.
    Node(NodeId),
    // No root: the group's roots come first in the parent.
    Nothing,
}

impl SlotTable {
    // What the composition emitted in `parent` before the group at
    // `group_index`, searched in the groups around it from the closest out
    // to the group that emitted `parent`. `None` when the table cannot
    // tell.
    pub(in crate::slot) fn root_before_group(
        &self,
        group_index: usize,
        parent: NodeId,
    ) -> Option<RootBefore> {
        let mut child = group_index;
        loop {
            let parent_anchor = self.groups.get(child)?.parent_anchor;
            if !parent_anchor.is_valid() {
                // `parent` is the host's root: nothing before the group
                // emitted into it.
                return Some(RootBefore::Nothing);
            }
            let owner = self.active_group_index(parent_anchor)?;
            if let Some(found) = self.root_before_child(owner, child, parent) {
                return Some(found);
            }
            child = owner;
        }
    }

    // What the group at `owner` emitted in `parent` before its direct child
    // at `child`: its own records and its earlier children's roots, in the
    // order it emitted them. `None` when it emitted neither a root of
    // `parent` nor `parent` itself.
    fn root_before_child(&self, owner: usize, child: usize, parent: NodeId) -> Option<RootBefore> {
        let own = self.own_node_records(self.groups.get(owner)?);
        let mut last = None;
        let mut next_own = 0;
        let mut children_before = 0;
        let mut index = owner + 1;
        loop {
            while let Some(record) = own
                .get(next_own)
                .filter(|record| record.child_groups_before <= children_before)
            {
                if record.id == parent {
                    last = Some(RootBefore::Nothing);
                } else if record.parent_id == Some(parent) {
                    last = Some(RootBefore::Node(record.id));
                }
                next_own += 1;
            }
            if index >= child {
                return last;
            }
            let sibling = self.groups.get(index)?;
            let first_parent = (sibling.subtree_node_count > 0)
                .then(|| self.nodes.get(sibling.node_start as usize))
                .flatten()
                .map(|record| record.parent_id);
            if first_parent == Some(Some(parent)) {
                let ControlFlow::Continue(()) =
                    self.try_for_each_root_in(index, Some(parent), &mut |id| {
                        last = Some(RootBefore::Node(id));
                        ControlFlow::<Infallible>::Continue(())
                    });
            }
            children_before += 1;
            index += (sibling.subtree_len as usize).max(1);
        }
    }
}

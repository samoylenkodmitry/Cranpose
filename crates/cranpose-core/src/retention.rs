use crate::collections::map::HashMap;
use crate::{
    slot::AnchorState, slot::DetachedSubtree, slot::NodeLifecycle, slot::SlotInvariantError,
    slot_storage::GroupKey, ScopeId, SlotTable,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetentionMode {
    #[default]
    DisposeWhenInactive,
    RetainWhenInactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RetainKey {
    pub(crate) parent_scope: Option<ScopeId>,
    pub(crate) key: GroupKey,
}

pub(crate) struct RetainedGroup {
    pub(crate) subtree: DetachedSubtree,
}

impl RetainedGroup {
    fn node_count(&self) -> usize {
        self.subtree.node_count()
    }

    fn scope_count(&self) -> usize {
        self.subtree.scope_count()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RetentionDebugStats {
    pub(crate) subtree_count: usize,
    pub(crate) group_count: usize,
    pub(crate) node_count: usize,
    pub(crate) scope_count: usize,
}

#[derive(Default)]
pub(crate) struct RetentionManager {
    groups: HashMap<RetainKey, RetainedGroup>,
}

impl RetentionManager {
    pub(crate) fn take(&mut self, key: RetainKey) -> Option<DetachedSubtree> {
        let mut retained = self.groups.remove(&key)?;
        retained.subtree.mark_nodes_active();
        Some(retained.subtree)
    }

    pub(crate) fn insert(&mut self, key: RetainKey, mut subtree: DetachedSubtree) {
        subtree.mark_nodes_retained_detached();
        self.groups.insert(key, RetainedGroup { subtree });
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub(crate) fn debug_stats(&self) -> RetentionDebugStats {
        RetentionDebugStats {
            subtree_count: self.groups.len(),
            group_count: self
                .groups
                .values()
                .map(|retained| retained.subtree.group_count())
                .sum(),
            node_count: self.groups.values().map(RetainedGroup::node_count).sum(),
            scope_count: self.groups.values().map(RetainedGroup::scope_count).sum(),
        }
    }

    pub(crate) fn into_subtrees(self) -> Vec<DetachedSubtree> {
        self.groups
            .into_values()
            .map(|retained| retained.subtree)
            .collect()
    }

    pub(crate) fn subtrees(&self) -> impl Iterator<Item = &DetachedSubtree> + '_ {
        self.groups.values().map(|retained| &retained.subtree)
    }

    pub(crate) fn subtrees_mut(&mut self) -> impl Iterator<Item = &mut DetachedSubtree> + '_ {
        self.groups
            .values_mut()
            .map(|retained| &mut retained.subtree)
    }

    pub(crate) fn validate(&self, table: &SlotTable) -> Result<(), SlotInvariantError> {
        for retained in self.groups.values() {
            let subtree = &retained.subtree;
            if subtree.root_parent_anchor().is_valid() {
                return Err(SlotInvariantError::RetainedRootHasActiveParent {
                    root_key: subtree.root_key(),
                    parent_anchor: subtree.root_parent_anchor(),
                });
            }

            for anchor in subtree.group_anchors() {
                if let Some(AnchorState::Active(active_index)) = table.anchor_state(anchor) {
                    return Err(SlotInvariantError::RetainedSubtreeAnchorStillActive {
                        root_key: subtree.root_key(),
                        anchor,
                        active_index,
                    });
                }
            }

            for (node_id, lifecycle) in subtree.node_states() {
                if lifecycle != NodeLifecycle::RetainedDetached {
                    return Err(SlotInvariantError::RetainedNodeLifecycleMismatch {
                        root_key: subtree.root_key(),
                        node_id,
                        actual: lifecycle,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn debug_verify(&self, table: &SlotTable) {
        if crate::slot_validation_diagnostics_enabled() {
            if let Err(err) = self.validate(table) {
                panic!("retention invariant violation: {err:?}");
            }
        }
    }
}

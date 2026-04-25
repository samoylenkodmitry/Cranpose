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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionBudget {
    pub max_retained_subtrees: Option<usize>,
    pub max_retained_bytes: Option<usize>,
    pub max_age_passes: Option<u64>,
}

impl RetentionBudget {
    pub const UNBOUNDED: Self = Self {
        max_retained_subtrees: None,
        max_retained_bytes: None,
        max_age_passes: None,
    };
}

impl Default for RetentionBudget {
    fn default() -> Self {
        Self::UNBOUNDED
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetentionEvictionPolicy {
    #[default]
    LeastRecentlyDetached,
    LeastRecentlyRestored,
    LargestFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub budget: RetentionBudget,
    pub eviction: RetentionEvictionPolicy,
}

impl RetentionPolicy {
    pub const UNBOUNDED: Self = Self {
        budget: RetentionBudget::UNBOUNDED,
        eviction: RetentionEvictionPolicy::LeastRecentlyDetached,
    };
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::UNBOUNDED
    }
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

    fn payload_count(&self) -> usize {
        self.subtree.payload_count()
    }

    fn scope_count(&self) -> usize {
        self.subtree.scope_count()
    }

    fn anchor_count(&self) -> usize {
        self.subtree.anchor_count()
    }

    fn heap_bytes(&self) -> usize {
        self.subtree.heap_bytes()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RetentionDebugStats {
    pub(crate) subtree_count: usize,
    pub(crate) group_count: usize,
    pub(crate) payload_count: usize,
    pub(crate) node_count: usize,
    pub(crate) scope_count: usize,
    pub(crate) anchor_count: usize,
    pub(crate) heap_bytes: usize,
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
        assert!(
            !self.groups.contains_key(&key),
            "retained subtree key collision: parent_scope={:?} key={:?}",
            key.parent_scope,
            key.key,
        );
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
            payload_count: self.groups.values().map(RetainedGroup::payload_count).sum(),
            node_count: self.groups.values().map(RetainedGroup::node_count).sum(),
            scope_count: self.groups.values().map(RetainedGroup::scope_count).sum(),
            anchor_count: self.groups.values().map(RetainedGroup::anchor_count).sum(),
            heap_bytes: self.groups.values().map(RetainedGroup::heap_bytes).sum(),
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
        for (key, retained) in &self.groups {
            let subtree = &retained.subtree;
            if subtree.root_key() != key.key {
                return Err(SlotInvariantError::RetainedRootKeyMismatch {
                    parent_scope: key.parent_scope,
                    expected: key.key,
                    actual: subtree.root_key(),
                });
            }

            if subtree.root_parent_anchor().is_valid() {
                return Err(SlotInvariantError::RetainedRootHasActiveParent {
                    root_key: subtree.root_key(),
                    parent_anchor: subtree.root_parent_anchor(),
                });
            }

            #[cfg(any(test, debug_assertions))]
            subtree.validate_detached()?;

            for anchor in subtree.group_anchors() {
                match table.anchor_state(anchor) {
                    Some(AnchorState::Detached) => {}
                    Some(AnchorState::Active(active_index)) => {
                        return Err(SlotInvariantError::RetainedSubtreeAnchorStillActive {
                            root_key: subtree.root_key(),
                            anchor,
                            active_index,
                        });
                    }
                    actual => {
                        return Err(SlotInvariantError::RetainedAnchorStateMismatch {
                            root_key: subtree.root_key(),
                            anchor,
                            actual,
                        });
                    }
                }
            }

            for scope_id in subtree.scope_ids_iter() {
                if let Some(active_anchor) = table.scope_index_anchor(scope_id) {
                    return Err(SlotInvariantError::RetainedScopeStillActive {
                        root_key: subtree.root_key(),
                        scope_id,
                        active_anchor,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_budget_default_is_unbounded() {
        assert_eq!(RetentionBudget::default(), RetentionBudget::UNBOUNDED);
        assert_eq!(RetentionBudget::default().max_retained_subtrees, None);
        assert_eq!(RetentionBudget::default().max_retained_bytes, None);
        assert_eq!(RetentionBudget::default().max_age_passes, None);
    }

    #[test]
    fn retention_policy_default_uses_unbounded_budget_and_detach_lru() {
        assert_eq!(RetentionPolicy::default(), RetentionPolicy::UNBOUNDED);
        assert_eq!(
            RetentionPolicy::default().budget,
            RetentionBudget::UNBOUNDED
        );
        assert_eq!(
            RetentionPolicy::default().eviction,
            RetentionEvictionPolicy::LeastRecentlyDetached
        );
    }

    #[test]
    fn retention_budget_can_express_all_limits() {
        let budget = RetentionBudget {
            max_retained_subtrees: Some(3),
            max_retained_bytes: Some(4096),
            max_age_passes: Some(5),
        };

        assert_eq!(budget.max_retained_subtrees, Some(3));
        assert_eq!(budget.max_retained_bytes, Some(4096));
        assert_eq!(budget.max_age_passes, Some(5));
    }
}

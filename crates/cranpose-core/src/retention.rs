use crate::collections::map::HashMap;
use crate::{
    slot::AnchorState, slot::DetachedSubtree, slot::NodeLifecycle, slot::SlotInvariantError,
    slot_storage::GroupKey, ScopeId, SlotTable,
};
use std::cmp::Ordering;

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
    detached_pass: u64,
    detached_order: u64,
    last_restored_order: u64,
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
    pub(crate) evictions_total: usize,
}

pub(crate) struct RetentionManager {
    groups: HashMap<RetainKey, RetainedGroup>,
    restored_at_by_key: HashMap<RetainKey, u64>,
    policy: RetentionPolicy,
    pass_clock: u64,
    operation_clock: u64,
    evictions_total: usize,
}

impl Default for RetentionManager {
    fn default() -> Self {
        Self::new(RetentionPolicy::default())
    }
}

impl RetentionManager {
    pub(crate) fn new(policy: RetentionPolicy) -> Self {
        Self {
            groups: HashMap::default(),
            restored_at_by_key: HashMap::default(),
            policy,
            pass_clock: 0,
            operation_clock: 0,
            evictions_total: 0,
        }
    }

    pub(crate) fn set_policy(&mut self, policy: RetentionPolicy) {
        self.policy = policy;
    }

    pub(crate) fn take(&mut self, key: RetainKey) -> Option<DetachedSubtree> {
        let restored_order = self.tick_operation();
        let mut retained = self.groups.remove(&key)?;
        self.restored_at_by_key.insert(key, restored_order);
        retained.subtree.mark_nodes_active();
        Some(retained.subtree)
    }

    pub(crate) fn insert(
        &mut self,
        key: RetainKey,
        mut subtree: DetachedSubtree,
    ) -> Vec<DetachedSubtree> {
        assert!(
            !self.groups.contains_key(&key),
            "retained subtree key collision: parent_scope={:?} key={:?}",
            key.parent_scope,
            key.key,
        );
        let detached_order = self.tick_operation();
        let last_restored_order = self.restored_at_by_key.remove(&key).unwrap_or_default();
        subtree.mark_nodes_retained_detached();
        self.groups.insert(
            key,
            RetainedGroup {
                subtree,
                detached_pass: self.pass_clock,
                detached_order,
                last_restored_order,
            },
        );
        self.evict_to_budget()
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
            evictions_total: self.evictions_total,
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

    pub(crate) fn evictions_total(&self) -> usize {
        self.evictions_total
    }

    pub(crate) fn advance_pass(&mut self) -> Vec<DetachedSubtree> {
        self.pass_clock = self.pass_clock.saturating_add(1);
        self.evict_to_budget()
    }

    fn tick_operation(&mut self) -> u64 {
        self.operation_clock = self.operation_clock.saturating_add(1);
        self.operation_clock
    }

    fn evict_to_budget(&mut self) -> Vec<DetachedSubtree> {
        let mut evicted = Vec::new();
        while let Some(key) = self.budget_eviction_key() {
            let Some(retained) = self.groups.remove(&key) else {
                break;
            };
            self.restored_at_by_key.remove(&key);
            self.evictions_total = self.evictions_total.saturating_add(1);
            evicted.push(retained.subtree);
        }
        evicted
    }

    fn budget_eviction_key(&self) -> Option<RetainKey> {
        if self.groups.is_empty() {
            return None;
        }

        if let Some(max_age_passes) = self.policy.budget.max_age_passes {
            if let Some(key) = self.age_eviction_key(max_age_passes) {
                return Some(key);
            }
        }

        let over_count = self
            .policy
            .budget
            .max_retained_subtrees
            .is_some_and(|max| self.groups.len() > max);
        let over_bytes = self
            .policy
            .budget
            .max_retained_bytes
            .is_some_and(|max| self.retained_heap_bytes() > max);

        (over_count || over_bytes)
            .then(|| self.policy_eviction_key())
            .flatten()
    }

    fn age_eviction_key(&self, max_age_passes: u64) -> Option<RetainKey> {
        self.groups
            .iter()
            .filter(|(_, retained)| {
                self.pass_clock.saturating_sub(retained.detached_pass) > max_age_passes
            })
            .min_by(|(left_key, left), (right_key, right)| {
                left.detached_pass
                    .cmp(&right.detached_pass)
                    .then_with(|| left.detached_order.cmp(&right.detached_order))
                    .then_with(|| retain_key_cmp(left_key, right_key))
            })
            .map(|(key, _)| *key)
    }

    fn policy_eviction_key(&self) -> Option<RetainKey> {
        match self.policy.eviction {
            RetentionEvictionPolicy::LeastRecentlyDetached => self.least_recently_detached_key(),
            RetentionEvictionPolicy::LeastRecentlyRestored => self.least_recently_restored_key(),
            RetentionEvictionPolicy::LargestFirst => self.largest_first_key(),
        }
    }

    fn least_recently_detached_key(&self) -> Option<RetainKey> {
        self.groups
            .iter()
            .min_by(|(left_key, left), (right_key, right)| {
                left.detached_order
                    .cmp(&right.detached_order)
                    .then_with(|| retain_key_cmp(left_key, right_key))
            })
            .map(|(key, _)| *key)
    }

    fn least_recently_restored_key(&self) -> Option<RetainKey> {
        self.groups
            .iter()
            .min_by(|(left_key, left), (right_key, right)| {
                left.last_restored_order
                    .cmp(&right.last_restored_order)
                    .then_with(|| left.detached_order.cmp(&right.detached_order))
                    .then_with(|| retain_key_cmp(left_key, right_key))
            })
            .map(|(key, _)| *key)
    }

    fn largest_first_key(&self) -> Option<RetainKey> {
        self.groups
            .iter()
            .max_by(|(left_key, left), (right_key, right)| {
                left.heap_bytes()
                    .cmp(&right.heap_bytes())
                    .then_with(|| retain_key_cmp(left_key, right_key))
            })
            .map(|(key, _)| *key)
    }

    fn retained_heap_bytes(&self) -> usize {
        self.groups.values().map(RetainedGroup::heap_bytes).sum()
    }
}

fn retain_key_cmp(left: &RetainKey, right: &RetainKey) -> Ordering {
    (
        left.parent_scope,
        left.key.static_key,
        left.key.explicit_key,
        left.key.ordinal,
    )
        .cmp(&(
            right.parent_scope,
            right.key.static_key,
            right.key.explicit_key,
            right.key.ordinal,
        ))
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

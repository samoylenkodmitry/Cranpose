use crate::collections::map::{HashMap, HashSet};
use crate::{slot_storage::GroupKey, slot_table::DetachedSubtree, NodeId, ScopeId};

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
    retained_nodes: Vec<NodeId>,
    scope_ids: Vec<ScopeId>,
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
    nodes: HashSet<NodeId>,
    scopes: HashMap<ScopeId, RetainKey>,
}

impl RetentionManager {
    pub(crate) fn take(&mut self, key: RetainKey) -> Option<DetachedSubtree> {
        let retained = self.groups.remove(&key)?;
        for node_id in &retained.retained_nodes {
            self.nodes.remove(node_id);
        }
        for scope_id in &retained.scope_ids {
            self.scopes.remove(scope_id);
        }
        Some(retained.subtree)
    }

    pub(crate) fn insert(&mut self, key: RetainKey, subtree: DetachedSubtree) {
        let retained_nodes = subtree.node_ids();
        let scope_ids = subtree.scope_ids();
        self.nodes.extend(retained_nodes.iter().copied());
        for &scope_id in &scope_ids {
            self.scopes.insert(scope_id, key);
        }
        self.groups.insert(
            key,
            RetainedGroup {
                subtree,
                retained_nodes,
                scope_ids,
            },
        );
    }

    pub(crate) fn clear(&mut self) {
        self.groups.clear();
        self.nodes.clear();
        self.scopes.clear();
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
            node_count: self.nodes.len(),
            scope_count: self.scopes.len(),
        }
    }
}

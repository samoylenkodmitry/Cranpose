// Disposition of detached subtrees: retain reusable groups, split nested
// movables out of leaving groups, and dispose the rest.

use smallvec::SmallVec;

use crate::{
    Command, ComposerCore, NodeError, NodeId, RecomposeScope, RetentionMode, ScopeId, SlotsHost,
    retention::RetainKey,
};

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl ComposerCore {
    pub(super) fn dispose_detached_nodes(&self, nodes: &[NodeId]) {
        if nodes.is_empty() {
            return;
        }
        let mut commands = self.commands.borrow_mut();
        for &id in nodes {
            commands.push_dispose_detached_node(id);
        }
    }

    pub(super) fn handle_detached_children_in_host(
        &self,
        slots_host: &SlotsHost,
        parent_scope: Option<ScopeId>,
        detached: Vec<crate::slot::DetachedSubtree>,
    ) -> Result<(), NodeError> {
        for mut subtree in detached {
            let is_movable_root = subtree
                .root_key_checked()
                .is_some_and(crate::slot::GroupKey::is_movable);
            let retention_mode = if is_movable_root {
                RetentionMode::RetainWhenInactive
            } else {
                subtree
                    .root_scope()
                    .map_or_default(RecomposeScope::retention_mode)
            };
            let nested_movables =
                if !is_movable_root && retention_mode == RetentionMode::DisposeWhenInactive {
                    subtree.split_off_unowned_nested_movables()
                } else {
                    subtree.split_off_nested_movables()
                };
            for movable in nested_movables {
                self.handle_detached_movable_in_host(slots_host, movable)?;
            }
            if is_movable_root {
                self.handle_detached_movable_in_host(slots_host, subtree)?;
                continue;
            }
            match retention_mode {
                RetentionMode::DisposeWhenInactive => {
                    self.dispose_detached_subtree_in_host(slots_host, subtree)?;
                }
                RetentionMode::RetainWhenInactive => {
                    self.retain_detached_subtree_in_host(slots_host, parent_scope, subtree)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn retain_detached_subtree_in_host(
        &self,
        slots_host: &SlotsHost,
        parent_scope: Option<ScopeId>,
        subtree: crate::slot::DetachedSubtree,
    ) -> Result<(), NodeError> {
        let Some(root_key) = subtree.root_key_checked() else {
            log::error!("retention rejected detached subtree without a root group");
            self.dispose_detached_subtree_in_host(slots_host, subtree)?;
            return Ok(());
        };
        let root_nodes = subtree.collect_checked_root_nodes("retention");
        let mut detaches = SmallVec::<[(NodeId, NodeId); 4]>::new();
        {
            let mut applier = self.applier.borrow_dyn();
            for &root in &root_nodes {
                if let Some(parent_id) = applier.get_mut(root)?.parent() {
                    detaches.push((parent_id, root));
                }
            }
        }
        subtree.scopes().for_each(RecomposeScope::deactivate);
        if !detaches.is_empty() {
            let mut commands = self.commands.borrow_mut();
            for (parent_id, child_id) in detaches {
                commands.push(Command::DetachChild {
                    parent_id,
                    child_id,
                });
            }
        }
        let evicted = self.shared_state.insert_retained(
            slots_host,
            RetainKey::for_group(parent_scope, root_key),
            subtree,
            root_key.is_movable(),
        );
        for subtree in evicted {
            self.dispose_detached_subtree_in_host(slots_host, subtree)?;
        }
        Ok(())
    }

    pub(super) fn evict_retained_subtrees_for_host(
        &self,
        slots_host: &SlotsHost,
    ) -> Result<(), NodeError> {
        if !self.shared_state.retains_any() {
            return Ok(());
        }
        let evicted = self.shared_state.advance_retention_pass(slots_host);
        for subtree in evicted {
            self.dispose_detached_subtree_in_host(slots_host, subtree)?;
        }
        Ok(())
    }

    pub(super) fn dispose_detached_subtree_in_host(
        &self,
        slots_host: &SlotsHost,
        subtree: crate::slot::DetachedSubtree,
    ) -> Result<(), NodeError> {
        let root_nodes = subtree.collect_checked_root_nodes("disposal");
        {
            let mut applier = self.applier.borrow_dyn();
            for &root in &root_nodes {
                applier.get_mut(root)?;
            }
        }
        subtree.scopes().for_each(RecomposeScope::deactivate);
        self.dispose_detached_nodes(&root_nodes);
        slots_host.with_table_and_lifecycle_mut(|table, lifecycle| {
            table.invalidate_detached_subtree_anchors(&subtree);
            lifecycle.queue_subtree_disposal(subtree);
        });
        Ok(())
    }
}

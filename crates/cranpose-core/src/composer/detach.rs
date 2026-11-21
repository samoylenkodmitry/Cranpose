// Disposition of detached subtrees: retain reusable groups, split nested
// movables out of leaving groups, and dispose the rest.

use super::movable_owners::movable_hooks;
use crate::{
    ComposerCore, NodeError, NodeId, RecomposeScope, RetentionMode, ScopeId, SlotsHost,
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
        let hooks = movable_hooks();
        for mut subtree in detached {
            let retention_mode = subtree
                .root_scope()
                .map_or_default(RecomposeScope::retention_mode);
            if let Some(hooks) = hooks {
                let Some(rest) =
                    (hooks.handle_detached_child)(self, slots_host, subtree, retention_mode)?
                else {
                    continue;
                };
                subtree = rest;
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
        let mut first_detach = None;
        let mut extra_detaches = Vec::new();
        {
            let mut applier = self.applier.borrow_dyn();
            for &root in &root_nodes {
                if let Some(parent_id) = applier.get_mut(root)?.parent() {
                    if first_detach.is_none() {
                        first_detach = Some((parent_id, root));
                    } else {
                        extra_detaches.push((parent_id, root));
                    }
                }
            }
        }
        subtree.scopes().for_each(RecomposeScope::deactivate);
        if first_detach.is_some() {
            let mut commands = self.commands.borrow_mut();
            for (parent_id, child_id) in first_detach.into_iter().chain(extra_detaches) {
                commands.push_detach_child(parent_id, child_id);
            }
        }
        let pin = if root_key.is_movable() {
            crate::retention::RetentionPin::Pinned
        } else {
            crate::retention::RetentionPin::Unpinned
        };
        let evicted = self.shared_state.insert_retained(
            slots_host,
            RetainKey::for_group(parent_scope, root_key),
            subtree,
            pin,
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

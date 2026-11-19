//! The state the composers of one runtime share: what each slots host
//! retains and moves, the hosts that live, the applier and the branch folds.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use super::{PendingMovable, movable_retain_key, slots_storage_key};
use crate::{
    ApplierHost, Key, NodeError, RecomposeScope, RetentionPolicy, SlotTable, SlotsHost,
    collections::map::{HashMap, HashSet},
    retention::{RetainKey, RetentionManager},
};

pub(crate) struct ComposePassGuard<'a> {
    state: &'a ComposerRuntimeState,
}

impl Drop for ComposePassGuard<'_> {
    fn drop(&mut self) {
        self.state.end_compose_pass();
    }
}

pub(crate) struct ComposerRuntimeState {
    retention_by_host: RefCell<HashMap<usize, RetentionManager>>,
    pending_movables_by_host: RefCell<HashMap<usize, Vec<PendingMovable>>>,
    retention_policy: Cell<RetentionPolicy>,
    live_hosts: RefCell<HashMap<usize, Weak<SlotsHost>>>,
    applier_host: RefCell<Option<Weak<dyn ApplierHost>>>,
    active_compose_passes: Cell<usize>,
    dropped_movable_owners: RefCell<Vec<Key>>,
    has_dropped_movable_owners: Cell<bool>,
    disposed_attached_movables: RefCell<HashSet<Key>>,
    has_disposed_attached_movables: Cell<bool>,
    pub(super) branch_folds: Rc<crate::slot::BranchFolds>,
    pub(crate) pausing: crate::pausable::Pausing,
}

impl Default for ComposerRuntimeState {
    fn default() -> Self {
        Self {
            retention_by_host: RefCell::new(HashMap::default()),
            pending_movables_by_host: RefCell::new(HashMap::default()),
            retention_policy: Cell::new(RetentionPolicy::default()),
            live_hosts: RefCell::new(HashMap::default()),
            applier_host: RefCell::new(None),
            active_compose_passes: Cell::new(0),
            dropped_movable_owners: RefCell::new(Vec::new()),
            has_dropped_movable_owners: Cell::new(false),
            disposed_attached_movables: RefCell::new(HashSet::default()),
            has_disposed_attached_movables: Cell::new(false),
            branch_folds: Rc::default(),
            pausing: crate::pausable::Pausing::default(),
        }
    }
}

impl ComposerRuntimeState {
    pub(crate) fn begin_compose_pass(&self) {
        self.active_compose_passes
            .set(self.active_compose_passes.get() + 1);
    }

    pub(crate) fn end_compose_pass(&self) {
        self.active_compose_passes
            .set(self.active_compose_passes.get().saturating_sub(1));
    }

    pub(crate) fn enter_compose_pass(&self) -> ComposePassGuard<'_> {
        self.begin_compose_pass();
        ComposePassGuard { state: self }
    }

    pub(crate) fn clear_host_storage_key(&self, host_key: usize, table: &mut SlotTable) {
        self.retention_by_host.borrow_mut().remove(&host_key);
        self.pending_movables_by_host.borrow_mut().remove(&host_key);
        self.live_hosts.borrow_mut().remove(&host_key);
        if self.has_disposed_attached_movables.get() {
            self.forget_disposed_attached_movables_in_table(table);
        }
        table.release_scopes();
    }

    #[cold]
    #[inline(never)]
    fn forget_disposed_attached_movables_in_table(&self, table: &SlotTable) {
        let mut disposed = self.disposed_attached_movables.borrow_mut();
        disposed.retain(|&id| !table.movable_is_attached(id));
        if disposed.is_empty() {
            self.has_disposed_attached_movables.set(false);
        }
    }

    pub(crate) fn force_recompose_retained_scopes(&self, host_key: usize) {
        if let Some(retention) = self.retention_by_host.borrow().get(&host_key) {
            for scope in retention
                .subtrees()
                .flat_map(crate::slot::DetachedSubtree::scopes)
            {
                scope.force_recompose();
            }
        }
    }

    pub(crate) fn bind_applier_host(&self, applier: &Rc<dyn ApplierHost>) {
        *self.applier_host.borrow_mut() = Some(Rc::downgrade(applier));
    }

    pub(crate) fn has_live_applier_host(&self) -> bool {
        self.applier_host
            .borrow()
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some()
    }

    pub(crate) fn bind_slots_host(self: &Rc<Self>, host: &Rc<SlotsHost>) {
        host.bind_runtime_state(self);
        self.live_hosts
            .borrow_mut()
            .insert(host.storage_key(), Rc::downgrade(host));
    }

    pub(crate) fn retains_any(&self) -> bool {
        !self.retention_by_host.borrow().is_empty()
    }

    pub(crate) fn set_retention_policy(&self, policy: RetentionPolicy) {
        self.retention_policy.set(policy);
    }

    pub(crate) fn retention_policy(&self) -> RetentionPolicy {
        self.retention_policy.get()
    }

    pub(crate) fn take_retained(
        &self,
        host: &Rc<SlotsHost>,
        key: RetainKey,
        preflight: impl FnOnce(&mut crate::slot::DetachedSubtree) -> bool,
    ) -> Option<crate::slot::DetachedSubtree> {
        let host_key = slots_storage_key(host);
        let mut retention = self.retention_by_host.borrow_mut();
        let subtree = retention
            .get_mut(&host_key)?
            .take_after_restore_preflight(key, preflight);
        remove_empty_retention_host(&mut retention, host_key);
        subtree
    }

    pub(crate) fn insert_retained(
        &self,
        host: &Rc<SlotsHost>,
        key: RetainKey,
        subtree: crate::slot::DetachedSubtree,
        pinned: bool,
    ) -> Vec<crate::slot::DetachedSubtree> {
        let policy = self.retention_policy();
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let manager = retention_by_host
            .entry(slots_storage_key(host))
            .or_insert_with(|| RetentionManager::new(policy));
        manager.set_policy(policy);
        if pinned {
            manager.insert_pinned(key, subtree)
        } else {
            manager.insert(key, subtree)
        }
    }

    pub(crate) fn take_retained_movable(
        &self,
        id: Key,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        self.take_retained_movable_by_key(movable_retain_key(id))
    }

    fn take_retained_movable_by_key(
        &self,
        key: RetainKey,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let (&host_key, manager) = retention_by_host
            .iter_mut()
            .find(|(_, manager)| manager.contains(key))?;
        let host = self.host_for_storage_key(host_key)?;
        let subtree = manager.take(key)?;
        remove_empty_retention_host(&mut retention_by_host, host_key);
        Some((host, subtree))
    }

    // Takes a movable's retained content out of a slot table other than the
    // one composing, which is how it crosses a subcomposition.
    pub(crate) fn take_retained_movable_elsewhere(
        &self,
        besides: &Rc<SlotsHost>,
        key: RetainKey,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        let besides_key = slots_storage_key(besides);
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let (&host_key, manager) = retention_by_host
            .iter_mut()
            .find(|(host_key, manager)| **host_key != besides_key && manager.contains(key))?;
        let host = self.host_for_storage_key(host_key)?;
        let subtree = manager.take(key)?;
        remove_empty_retention_host(&mut retention_by_host, host_key);
        Some((host, subtree))
    }

    pub(crate) fn on_movable_owner_dropped(&self, id: Key) {
        if self.active_compose_passes.get() > 0 {
            self.dropped_movable_owners.borrow_mut().push(id);
            self.has_dropped_movable_owners.set(true);
            return;
        }
        self.dispose_dropped_movable_outside_pass(id);
    }

    pub(crate) fn has_dropped_movable_owners(&self) -> bool {
        self.has_dropped_movable_owners.get()
    }

    pub(crate) fn take_dropped_movable_owners(&self) -> Vec<Key> {
        self.has_dropped_movable_owners.set(false);
        std::mem::take(&mut *self.dropped_movable_owners.borrow_mut())
    }

    pub(crate) fn note_disposed_attached_movable(&self, id: Key) {
        self.disposed_attached_movables.borrow_mut().insert(id);
        self.has_disposed_attached_movables.set(true);
    }

    pub(crate) fn take_disposed_attached_movable(&self, id: Key) -> bool {
        if !self.has_disposed_attached_movables.get() {
            return false;
        }
        self.take_disposed_attached_movable_slow(id)
    }

    #[cold]
    #[inline(never)]
    fn take_disposed_attached_movable_slow(&self, id: Key) -> bool {
        let mut disposed = self.disposed_attached_movables.borrow_mut();
        let removed = disposed.remove(&id);
        if disposed.is_empty() {
            self.has_disposed_attached_movables.set(false);
        }
        removed
    }

    #[cold]
    #[inline(never)]
    fn dispose_dropped_movable_outside_pass(&self, id: Key) {
        let Some((host, subtree)) = self.take_retained_movable(id) else {
            if self.host_holding_movable(id).is_some() {
                self.note_disposed_attached_movable(id);
            }
            return;
        };
        let _teardown = crate::runtime::enter_state_teardown_scope();
        self.dispose_detached_subtree_outside_pass(&host, subtree);
    }

    fn dispose_detached_subtree_outside_pass(
        &self,
        host: &Rc<SlotsHost>,
        subtree: crate::slot::DetachedSubtree,
    ) {
        subtree.scopes().for_each(RecomposeScope::deactivate);
        host.borrow_mut()
            .invalidate_detached_subtree_anchors(&subtree);
        let applier_host = self.applier_host.borrow().as_ref().and_then(Weak::upgrade);
        if let Some(applier_host) = applier_host {
            let mut disposal = crate::NodeDisposal::default();
            disposal.retain_subtree(subtree);
            if let Err(err) = applier_host.dispose_nodes(disposal) {
                log::error!("disposing orphaned movable content failed: {err}");
            }
        } else {
            host.with_table_and_lifecycle_mut(|_, lifecycle| {
                lifecycle.queue_subtree_disposal(subtree);
            });
            host.flush_pending_drops();
        }
    }

    /// Whether a movable's content is retained in any slot table, which is
    /// what a site waiting for it has to know: the content may have been let
    /// go by a parent composed in another host, such as a subcomposition.
    pub(crate) fn movable_retained_anywhere(&self, key: RetainKey) -> bool {
        self.retention_by_host
            .borrow()
            .values()
            .any(|manager| manager.contains(key))
    }

    /// The slot table a movable's content is attached to right now, if any.
    pub(crate) fn host_holding_movable(&self, id: Key) -> Option<Rc<SlotsHost>> {
        self.live_hosts
            .borrow()
            .values()
            .filter_map(Weak::upgrade)
            .find(|host| host.borrow().movable_is_attached(id))
    }

    pub(crate) fn record_pending_movable(&self, host: &Rc<SlotsHost>, pending: PendingMovable) {
        let mut by_host = self.pending_movables_by_host.borrow_mut();
        let sites = by_host.entry(slots_storage_key(host)).or_default();
        if sites
            .iter()
            .all(|site| site.placeholder != pending.placeholder)
        {
            sites.push(pending);
        }
    }

    pub(crate) fn take_pending_movables(&self, host: &Rc<SlotsHost>) -> Vec<PendingMovable> {
        self.pending_movables_by_host
            .borrow_mut()
            .remove(&slots_storage_key(host))
            .unwrap_or_default()
    }

    /// Every slot table with a site still waiting for movable content. The
    /// content may be let go by a parent composed in another table, so a pass
    /// that ends has to look wider than itself.
    pub(crate) fn hosts_awaiting_movables(&self) -> Vec<Rc<SlotsHost>> {
        let by_host = self.pending_movables_by_host.borrow();
        if by_host.is_empty() {
            return Vec::new();
        }
        let waiting = by_host
            .iter()
            .filter(|(_, sites)| !sites.is_empty())
            .map(|(host_key, _)| *host_key)
            .collect::<Vec<_>>();
        waiting
            .into_iter()
            .filter_map(|host_key| self.host_for_storage_key(host_key))
            .collect()
    }

    pub(crate) fn keep_pending_movables(&self, host: &Rc<SlotsHost>, sites: Vec<PendingMovable>) {
        if sites.is_empty() {
            return;
        }
        self.pending_movables_by_host
            .borrow_mut()
            .insert(slots_storage_key(host), sites);
    }

    pub(crate) fn advance_retention_pass(
        &self,
        host: &Rc<SlotsHost>,
    ) -> Vec<crate::slot::DetachedSubtree> {
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        if retention_by_host.is_empty() {
            return Vec::new();
        }
        let host_key = slots_storage_key(host);
        let policy = self.retention_policy();
        let Some(manager) = retention_by_host.get_mut(&host_key) else {
            return Vec::new();
        };
        manager.set_policy(policy);
        manager.advance_pass()
    }

    pub(crate) fn fill_slot_debug_snapshot(
        &self,
        host: &SlotsHost,
        snapshot: &mut crate::SlotDebugSnapshot,
    ) {
        let retention = self.retention_debug_stats(host.storage_key());
        snapshot.retained_subtree_count = retention.subtree_count;
        snapshot.retained_group_count = retention.group_count;
        snapshot.retained_payload_count = retention.payload_count;
        snapshot.retained_node_count = retention.node_count;
        snapshot.retained_scope_count = retention.scope_count;
    }

    pub(crate) fn slot_retention_debug_stats(
        &self,
        host: &SlotsHost,
    ) -> crate::slot::SlotRetentionDebugStats {
        let retention = self.retention_debug_stats(host.storage_key());
        crate::slot::SlotRetentionDebugStats {
            retained_subtree_count: retention.subtree_count,
            retained_group_count: retention.group_count,
            retained_payload_count: retention.payload_count,
            retained_node_count: retention.node_count,
            retained_scope_count: retention.scope_count,
            retained_anchor_count: retention.anchor_count,
            retained_heap_bytes: retention.heap_bytes,
            retained_evictions_total: retention.evictions_total,
        }
    }

    pub(crate) fn compact_table_identity_storage_for_host(
        &self,
        host: &SlotsHost,
        table: &mut SlotTable,
        compact_anchors: bool,
        compact_payloads: bool,
    ) {
        if !compact_anchors && !compact_payloads {
            return;
        }

        let host_key = host.storage_key();
        let mut retention = self.retention_by_host.borrow_mut();
        if let Some(retained) = retention.get_mut(&host_key) {
            if compact_anchors {
                table.compact_anchor_registry_storage(Some(&mut *retained));
            }
            if compact_payloads {
                table.compact_payload_anchor_registry_storage(Some(&mut *retained));
            }
        } else {
            if compact_anchors {
                table.compact_anchor_registry_storage(None);
            }
            if compact_payloads {
                table.compact_payload_anchor_registry_storage(None);
            }
        }
    }

    pub(crate) fn clear_host(&self, host: &SlotsHost, table: &mut SlotTable) {
        let host_key = host.storage_key();
        debug_assert!(
            self.host_retention_is_empty(host),
            "host retention must be drained before clearing host ownership"
        );
        self.clear_host_storage_key(host_key, table);
    }

    fn deactivate_and_queue_subtrees(
        &self,
        retention: RetentionManager,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) {
        for subtree in retention.into_subtrees() {
            for scope in subtree.scopes() {
                scope.deactivate();
            }
            table.invalidate_detached_subtree_anchors(&subtree);
            lifecycle.queue_subtree_disposal(subtree);
        }
    }

    pub(crate) fn dispose_host_on_drop(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) {
        let retention = self.retention_by_host.borrow_mut().remove(&host_key);
        let applier_host = self.applier_host.borrow().as_ref().and_then(Weak::upgrade);
        let Some(applier_host) = applier_host else {
            if let Some(retention) = retention {
                self.deactivate_and_queue_subtrees(retention, table, lifecycle);
            }
            self.clear_host_storage_key(host_key, table);
            lifecycle.dispose_slot_table(table);
            return;
        };
        let retained_subtrees: Vec<_> = retention.map_or_default(RetentionManager::into_subtrees);
        if retained_subtrees.is_empty() {
            self.clear_host_storage_key(host_key, table);
            lifecycle.dispose_slot_table(table);
            return;
        }
        for subtree in &retained_subtrees {
            subtree.scopes().for_each(RecomposeScope::deactivate);
            table.invalidate_detached_subtree_anchors(subtree);
        }
        self.clear_host_storage_key(host_key, table);
        // Queue the host's own slot payloads underneath the retained subtrees
        // in the same NodeDisposal so every retained node unmounts before any
        // remembered state or resource in this host drops.
        let mut disposal = crate::NodeDisposal::default();
        disposal.append_drops(table.take_all_drops());
        disposal.append_lifecycle(std::mem::take(lifecycle));
        for subtree in retained_subtrees {
            disposal.retain_subtree(subtree);
        }
        if let Err(err) = applier_host.dispose_nodes(disposal) {
            log::error!(
                "retained subtree disposal failed while dropping SlotsHost {host_key}: {err}"
            );
        }
    }

    pub(crate) fn dispose_retained_subtrees_for_host(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) -> Result<(), NodeError> {
        let retention = self.retention_by_host.borrow_mut().remove(&host_key);
        let applier_host = self.applier_host.borrow().as_ref().and_then(Weak::upgrade);
        if let Some(applier_host) = applier_host {
            let mut disposal = crate::NodeDisposal::default();
            if let Some(retention) = retention {
                for subtree in retention.into_subtrees() {
                    subtree.scopes().for_each(RecomposeScope::deactivate);
                    table.invalidate_detached_subtree_anchors(&subtree);
                    disposal.retain_subtree(subtree);
                }
            }
            applier_host.dispose_nodes(disposal)?;
        } else if let Some(retention) = retention {
            self.deactivate_and_queue_subtrees(retention, table, lifecycle);
        }
        Ok(())
    }

    pub(crate) fn abandon_retained_subtrees_for_host(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) {
        let Some(retention) = self.retention_by_host.borrow_mut().remove(&host_key) else {
            self.clear_host_storage_key(host_key, table);
            return;
        };
        self.deactivate_and_queue_subtrees(retention, table, lifecycle);
        self.clear_host_storage_key(host_key, table);
    }

    pub(crate) fn host_retention_is_empty(&self, host: &SlotsHost) -> bool {
        self.retention_by_host
            .borrow()
            .get(&host.storage_key())
            .is_none_or(RetentionManager::is_empty)
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn debug_verify_host(&self, host: &SlotsHost, table: &SlotTable) {
        if let Some(retention) = self.retention_by_host.borrow().get(&host.storage_key()) {
            retention.debug_verify(table);
        }
    }

    #[cfg(test)]
    pub(crate) fn validate_host_retention(
        &self,
        host: &SlotsHost,
        table: &SlotTable,
    ) -> Result<(), crate::slot::SlotInvariantError> {
        if let Some(retention) = self.retention_by_host.borrow().get(&host.storage_key()) {
            retention.validate(table)?;
        }
        Ok(())
    }

    pub(crate) fn host_for_storage_key(&self, storage_key: usize) -> Option<Rc<SlotsHost>> {
        self.live_hosts
            .borrow()
            .get(&storage_key)
            .and_then(Weak::upgrade)
    }

    #[cold]
    #[inline(never)]
    fn retention_debug_stats(&self, host_key: usize) -> crate::retention::RetentionDebugStats {
        self.retention_by_host
            .borrow()
            .get(&host_key)
            .map_or_default(RetentionManager::debug_stats)
    }
}

fn remove_empty_retention_host(
    retention_by_host: &mut HashMap<usize, RetentionManager>,
    host_key: usize,
) {
    if retention_by_host
        .get(&host_key)
        .is_some_and(|manager| manager.is_empty() && manager.evictions_total() == 0)
    {
        retention_by_host.remove(&host_key);
    }
}

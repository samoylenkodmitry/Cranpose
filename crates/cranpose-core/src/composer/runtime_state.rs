//! The state the composers of one runtime share: what each slots host
//! retains and moves, the hosts that live, the applier and the branch folds.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use super::{
    DroppedMovable, MovableOwners, PendingMovable, movable_owners::panic_invariant,
    movable_retain_key, slots_storage_key,
};
use crate::{
    ApplierHost, Key, RecomposeScope, RetentionPolicy, SlotTable, SlotsHost,
    collections::map::HashMap,
    retention::{RestoreRecord, RetainKey, RetentionManager},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostTableDisposal {
    Keep,
    DisposeWithRetained,
}

#[derive(Clone, Copy)]
enum HostFilter {
    Any,
    Only(usize),
    Except(usize),
}

pub(crate) struct PreparedRetainedDisposal {
    applier_host: Rc<dyn ApplierHost>,
    nodes: Vec<(crate::NodeId, u32)>,
    lifecycle: crate::slot::SlotLifecycleCoordinator,
}

impl PreparedRetainedDisposal {
    fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.lifecycle.pending_drops_len() == 0
    }

    pub(crate) fn dispose(self) -> Result<(), crate::NodeError> {
        self.applier_host
            .dispose_nodes(crate::NodeDisposal::from_host_parts(
                self.nodes,
                self.lifecycle,
            ))
    }
}

pub(crate) struct ComposerRuntimeState {
    retention_by_host: RefCell<HashMap<usize, RetentionManager>>,
    retained_groups: Cell<u32>,
    retention_policy: Cell<RetentionPolicy>,
    live_hosts: RefCell<HashMap<usize, Weak<SlotsHost>>>,
    applier_host: RefCell<Option<Weak<dyn ApplierHost>>>,
    pub(crate) movable_owners: MovableOwners,
    pub(super) branch_folds: Rc<crate::slot::BranchFolds>,
    pub(crate) pausing: crate::pausable::Pausing,
}

impl Default for ComposerRuntimeState {
    fn default() -> Self {
        Self {
            retention_by_host: RefCell::new(HashMap::default()),
            retained_groups: Cell::new(0),
            retention_policy: Cell::new(RetentionPolicy::default()),
            live_hosts: RefCell::new(HashMap::default()),
            applier_host: RefCell::new(None),
            movable_owners: MovableOwners::default(),
            branch_folds: Rc::default(),
            pausing: crate::pausable::Pausing::default(),
        }
    }
}

fn should_drop_empty_retention(manager: &RetentionManager) -> bool {
    manager.is_empty() && manager.evictions_total() == 0 && !manager.is_small_for_reuse()
}

impl ComposerRuntimeState {
    fn add_retained_groups(&self, count: usize) {
        let Some(delta) = u32::try_from(count).ok() else {
            panic_invariant("retained group count overflow");
        };
        let Some(next) = self.retained_groups.get().checked_add(delta) else {
            panic_invariant("retained group count overflow");
        };
        self.retained_groups.set(next);
    }

    fn sub_retained_groups(&self, count: usize) {
        let Some(delta) = u32::try_from(count).ok() else {
            panic_invariant("retained group count underflow");
        };
        let Some(next) = self.retained_groups.get().checked_sub(delta) else {
            panic_invariant("retained group count underflow");
        };
        self.retained_groups.set(next);
    }

    fn remove_host_retention(&self, host_key: usize) -> Option<RetentionManager> {
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        if retention_by_host.is_empty() {
            return None;
        }
        let removed = retention_by_host.remove(&host_key)?;
        if !removed.is_empty() {
            self.sub_retained_groups(removed.len());
        }
        Some(removed)
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn clear_host_storage_key(&self, host_key: usize, table: &mut SlotTable) {
        self.remove_host_retention(host_key);
        self.live_hosts.borrow_mut().remove(&host_key);
        self.movable_owners.clear_host(host_key, table);
        table.release_scopes();
    }

    pub(crate) fn force_recompose_retained_scopes(&self, host_key: usize) {
        if !self.retains_any() {
            return;
        }
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

    #[inline]
    pub(crate) fn retains_any(&self) -> bool {
        self.retained_groups.get() > 0
    }

    pub(crate) fn set_retention_policy(&self, policy: RetentionPolicy) {
        self.retention_policy.set(policy);
    }

    pub(crate) fn retention_policy(&self) -> RetentionPolicy {
        self.retention_policy.get()
    }

    #[inline(never)]
    pub(crate) fn take_retained(
        &self,
        host: &Rc<SlotsHost>,
        key: RetainKey,
        group_key: crate::slot::GroupKey,
    ) -> Option<crate::slot::DetachedSubtree> {
        if !self.retains_any() {
            return None;
        }
        let host_key = slots_storage_key(host);
        let mut retention = self.retention_by_host.borrow_mut();
        let manager = retention.get_mut(&host_key)?;
        let subtree = manager.take_after_restore_preflight(key, |subtree| {
            host.with_write_session(|slots| slots.retained_restore_ready(group_key, subtree))
        });
        if subtree.is_some() {
            self.sub_retained_groups(1);
        }
        if should_drop_empty_retention(manager) {
            retention.remove(&host_key);
        }
        subtree
    }

    pub(crate) fn insert_retained(
        &self,
        host: &SlotsHost,
        key: RetainKey,
        subtree: crate::slot::DetachedSubtree,
        pin: crate::retention::RetentionPin,
    ) -> Vec<crate::slot::DetachedSubtree> {
        let policy = self.retention_policy();
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let manager = retention_by_host
            .entry(slots_storage_key(host))
            .or_insert_with(|| RetentionManager::new(policy));
        manager.set_policy(policy);
        let before = manager.len();
        let evicted = manager.insert_with_pin(key, subtree, pin);
        let after = manager.len();
        if after >= before {
            self.add_retained_groups(after - before);
        } else {
            self.sub_retained_groups(before - after);
        }
        evicted
    }

    pub(crate) fn take_retained_movable(
        &self,
        id: Key,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        self.take_retained_movable_matching(
            movable_retain_key(id),
            HostFilter::Any,
            RestoreRecord::ClearForDisposal,
        )
    }

    pub(crate) fn take_retained_movable_in(
        &self,
        id: Key,
        owner_host: Option<usize>,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        let filter = match owner_host {
            Some(host_key) => HostFilter::Only(host_key),
            None => HostFilter::Any,
        };
        self.take_retained_movable_matching(
            movable_retain_key(id),
            filter,
            RestoreRecord::ClearForDisposal,
        )
    }

    // Takes a movable's retained content out of a slot table other than the
    // one composing, which is how it crosses a subcomposition.
    pub(crate) fn take_retained_movable_elsewhere(
        &self,
        besides: &Rc<SlotsHost>,
        key: RetainKey,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        let taken = self.take_retained_movable_matching(
            key,
            HostFilter::Except(slots_storage_key(besides)),
            RestoreRecord::Keep,
        )?;
        if let Some(id) = key.key.movable_id() {
            self.movable_owners.clear_dropped_owner_host(id);
        }
        Some(taken)
    }

    #[cold]
    #[inline(never)]
    fn take_retained_movable_matching(
        &self,
        key: RetainKey,
        filter: HostFilter,
        record: RestoreRecord,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        if !self.retains_any() {
            return None;
        }
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let host_key = match filter {
            HostFilter::Only(host_key) => host_key,
            HostFilter::Any => retention_by_host
                .iter()
                .find_map(|(&host_key, manager)| manager.contains(key).then_some(host_key))?,
            HostFilter::Except(besides_key) => {
                retention_by_host.iter().find_map(|(&host_key, manager)| {
                    (host_key != besides_key && manager.contains(key)).then_some(host_key)
                })?
            }
        };
        let manager = retention_by_host.get_mut(&host_key)?;
        if !manager.contains(key) {
            return None;
        }
        let host = self.host_for_storage_key(host_key)?;
        let subtree = manager.take_with_mode(key, record)?;
        self.sub_retained_groups(1);
        if should_drop_empty_retention(manager) {
            retention_by_host.remove(&host_key);
        }
        Some((host, subtree))
    }

    pub(crate) fn on_movable_owner_dropped(&self, id: Key, owner_host: Option<usize>) {
        let dropped = DroppedMovable { id, owner_host };
        if self.movable_owners.record_dropped_in_pass(dropped) {
            return;
        }
        self.dispose_dropped_movable_outside_pass(dropped);
    }

    #[cold]
    #[inline(never)]
    fn dispose_dropped_movable_outside_pass(&self, dropped: DroppedMovable) {
        let Some((host, subtree)) = self.take_retained_movable_in(dropped.id, dropped.owner_host)
        else {
            if self
                .host_holding_movable_in(dropped.id, dropped.owner_host)
                .is_some()
            {
                self.movable_owners.note_disposed_attached(dropped.id);
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
        if !self.retains_any() {
            return false;
        }
        self.retention_by_host
            .borrow()
            .values()
            .any(|manager| manager.contains(key))
    }

    /// The slot table a movable's content is attached to right now, if any.
    pub(crate) fn host_holding_movable(&self, id: Key) -> Option<Rc<SlotsHost>> {
        self.host_holding_movable_in(id, None)
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn host_holding_movable_in(
        &self,
        id: Key,
        owner_host: Option<usize>,
    ) -> Option<Rc<SlotsHost>> {
        if let Some(host_key) = owner_host {
            return self
                .host_for_storage_key(host_key)
                .filter(|host| host.borrow().movable_is_attached(id));
        }
        self.live_hosts
            .borrow()
            .values()
            .filter_map(Weak::upgrade)
            .find(|host| host.borrow().movable_is_attached(id))
    }

    pub(crate) fn record_pending_movable(&self, host: &Rc<SlotsHost>, pending: PendingMovable) {
        self.movable_owners
            .record_pending(slots_storage_key(host), pending);
    }

    pub(crate) fn has_pending_movable(&self, id: Key) -> bool {
        self.movable_owners.has_pending(id)
    }

    pub(crate) fn advance_retention_pass(
        &self,
        host: &SlotsHost,
    ) -> Vec<crate::slot::DetachedSubtree> {
        if !self.retains_any() {
            return Vec::new();
        }
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let host_key = slots_storage_key(host);
        let policy = self.retention_policy();
        let Some(manager) = retention_by_host.get_mut(&host_key) else {
            return Vec::new();
        };
        manager.set_policy(policy);
        let evicted = manager.advance_pass();
        self.sub_retained_groups(evicted.len());
        evicted
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

        let mut retention = self
            .retains_any()
            .then(|| self.retention_by_host.borrow_mut());
        let mut retained = retention
            .as_deref_mut()
            .and_then(|map| map.get_mut(&host.storage_key()));
        if compact_anchors {
            table.compact_anchor_registry_storage(retained.as_deref_mut());
        }
        if compact_payloads {
            table.compact_payload_anchor_registry_storage(retained);
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

    fn queue_retained_subtrees(
        retention: RetentionManager,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
        mut nodes: Option<&mut Vec<(crate::NodeId, u32)>>,
    ) {
        for subtree in retention.into_subtrees() {
            subtree.scopes().for_each(RecomposeScope::deactivate);
            table.invalidate_detached_subtree_anchors(&subtree);
            if let Some(roots) = nodes.as_deref_mut() {
                subtree.append_disposal_roots(roots);
            }
            lifecycle.queue_subtree_disposal(subtree);
        }
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn prepare_retained_disposal_for_host(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
        table_disposal: HostTableDisposal,
    ) -> Option<PreparedRetainedDisposal> {
        let retention = self.remove_host_retention(host_key);
        let applier_host = self.applier_host.borrow().as_ref().and_then(Weak::upgrade);
        let Some(applier_host) = applier_host else {
            if let Some(retention) = retention {
                Self::queue_retained_subtrees(retention, table, lifecycle, None);
            }
            return None;
        };
        if retention.is_none() && table_disposal == HostTableDisposal::Keep {
            return None;
        }
        let mut nodes = Vec::new();
        let mut disposal_lifecycle = crate::slot::SlotLifecycleCoordinator::default();
        let retained_subtrees = retention.map(RetentionManager::into_subtrees);
        if let Some(subtrees) = &retained_subtrees {
            for subtree in subtrees {
                subtree.scopes().for_each(RecomposeScope::deactivate);
                table.invalidate_detached_subtree_anchors(subtree);
            }
        }
        if table_disposal == HostTableDisposal::DisposeWithRetained {
            // Forget disposed-but-attached movables owned by other hosts before
            // `take_all_drops` clears this table's anchors and movable groups.
            self.movable_owners.forget_host(table);
            // Queue the host's own slot payloads underneath any retained
            // subtrees so retained nodes unmount first and the host's slot
            // drops run inside the applier host's teardown scope.
            table.release_scopes();
            disposal_lifecycle.queue_drops(table.take_all_drops());
            disposal_lifecycle.append(lifecycle);
        }
        if let Some(subtrees) = retained_subtrees {
            for subtree in subtrees {
                subtree.append_disposal_roots(&mut nodes);
                disposal_lifecycle.queue_subtree_disposal(subtree);
            }
        }
        Some(PreparedRetainedDisposal {
            applier_host,
            nodes,
            lifecycle: disposal_lifecycle,
        })
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn dispose_host_on_drop(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) {
        let prepared = self.prepare_retained_disposal_for_host(
            host_key,
            table,
            lifecycle,
            HostTableDisposal::DisposeWithRetained,
        );
        self.clear_host_storage_key(host_key, table);
        match prepared {
            Some(prepared) if !prepared.is_empty() => {
                if let Err(err) = prepared.dispose() {
                    log::error!(
                        "retained subtree disposal failed while dropping SlotsHost {host_key}: {err}"
                    );
                }
            }
            _ => lifecycle.dispose_slot_table(table),
        }
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn abandon_retained_subtrees_for_host(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) {
        if let Some(retention) = self.remove_host_retention(host_key) {
            Self::queue_retained_subtrees(retention, table, lifecycle, None);
        }
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
        let retention_by_host = self.retention_by_host.borrow();
        let total: usize = retention_by_host.values().map(RetentionManager::len).sum();
        debug_assert_eq!(
            self.retained_groups.get() as usize,
            total,
            "retained_groups counter diverged from retention_by_host"
        );
        if let Some(retention) = retention_by_host.get(&host.storage_key()) {
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
            .map_or_else(Default::default, RetentionManager::debug_stats)
    }
}

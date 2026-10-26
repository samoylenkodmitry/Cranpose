use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    rc::{Rc, Weak},
};

use crate::{
    Applier, NodeError, RecomposeScope, SlotDebugSnapshot, SlotTable, SlotTableDebugStats,
    composer, slot,
};

/// Holds a slot table into which a composition writes: the main composition has one, and each
/// subcomposition has its own.
pub struct SlotsHost {
    storage_key: Cell<usize>,
    pub(crate) inner: RefCell<SlotsHostInner>,
}

#[derive(Debug, Default)]
pub(crate) struct SlotPassOutcome {
    pub(crate) compacted: bool,
    pub(crate) compact_anchor_registry_storage: bool,
    pub(crate) compact_payload_storage: bool,
    pub(crate) trim_growth_slack: bool,
}

#[derive(Default)]
pub(crate) struct FinishedSlotPass {
    pub(crate) outcome: SlotPassOutcome,
    pub(crate) detached_root_children: Vec<slot::DetachedSubtree>,
}

pub(crate) struct SlotsHostInner {
    table: SlotTable,
    nested_hosts: Vec<Weak<SlotsHost>>,
    lifecycle: slot::SlotLifecycleCoordinator,
    runtime_state: Option<Rc<composer::ComposerRuntimeState>>,
    /// The table's storage capacity when the active pass began; `None`
    /// while no pass is active.
    pub(crate) active_pass: Option<usize>,
    /// The passes' writer state, `None` before the first pass. It stays in
    /// place between passes, each reusing its storage: moved, its hundreds
    /// of bytes cost a copy at each pass's start and end.
    pub(crate) pass_state: Option<slot::SlotWriteSessionState>,
}

impl Drop for SlotsHost {
    fn drop(&mut self) {
        let storage_key = self.storage_key.get();
        let inner = self.inner.get_mut();
        if let Some(state) = inner.runtime_state.clone() {
            if let Err(err) = state.dispose_retained_subtrees_for_host(
                storage_key,
                &mut inner.table,
                &mut inner.lifecycle,
            ) {
                log::error!(
                    "retained subtree disposal failed while dropping SlotsHost {storage_key}: {err}"
                );
                state.abandon_retained_subtrees_for_host(
                    storage_key,
                    &mut inner.table,
                    &mut inner.lifecycle,
                );
            } else {
                state.clear_host_storage_key(storage_key, &mut inner.table);
            }
        }
        inner.lifecycle.dispose_slot_table(&mut inner.table);
    }
}

impl SlotsHost {
    /// Returns a key that identifies the slot table the host holds. It changes when the table is
    /// replaced or taken out.
    pub fn storage_key(&self) -> usize {
        self.storage_key.get()
    }

    /// Creates a host around the slot table `storage`.
    pub fn new(storage: SlotTable) -> Self {
        let storage_key = storage.storage_id();
        Self {
            storage_key: Cell::new(storage_key),
            inner: RefCell::new(SlotsHostInner {
                table: storage,
                nested_hosts: Vec::new(),
                lifecycle: slot::SlotLifecycleCoordinator::default(),
                runtime_state: None,
                active_pass: None,
                pass_state: None,
            }),
        }
    }

    /// Records `nested` as a host composed inside this one, so that forgetting effects here reaches
    /// it too. Does nothing while this host is in use.
    pub fn note_nested_host(&self, nested: &Rc<Self>) {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return;
        };
        inner.nested_hosts.retain(|held| held.upgrade().is_some());
        if inner
            .nested_hosts
            .iter()
            .any(|held| held.upgrade().is_some_and(|host| Rc::ptr_eq(&host, nested)))
        {
            return;
        }
        inner.nested_hosts.push(Rc::downgrade(nested));
    }

    pub(crate) fn forget_effects(&self) -> bool {
        let (forgotten, nested, runtime_state) = {
            let Ok(mut inner) = self.inner.try_borrow_mut() else {
                return false;
            };
            if inner.active_pass.is_some() {
                return false;
            }
            let drops = inner.table.take_effect_drops();
            inner.nested_hosts.retain(|held| held.upgrade().is_some());
            let nested: Vec<Rc<Self>> = inner
                .nested_hosts
                .iter()
                .filter_map(Weak::upgrade)
                .collect();
            (drops, nested, inner.runtime_state.clone())
        };
        let mut any = !forgotten.is_empty();
        drop(forgotten);
        for host in nested {
            any |= host.forget_effects();
        }
        if any {
            self.borrow()
                .scopes()
                .for_each(RecomposeScope::force_recompose);
            if let Some(runtime_state) = runtime_state {
                runtime_state.force_recompose_retained_scopes(self.storage_key());
            }
        }
        any
    }

    pub(crate) fn bind_runtime_state(&self, state: &Rc<composer::ComposerRuntimeState>) {
        let mut inner = self.inner.borrow_mut();
        inner.runtime_state = Some(Rc::clone(state));
    }

    pub(crate) fn rebind_orphaned_runtime_state(
        &self,
        state: &Rc<composer::ComposerRuntimeState>,
    ) -> bool {
        let inner = self.inner.borrow();
        if inner.active_pass.is_some() {
            log::error!("cannot rebind SlotsHost during an active pass");
            return false;
        }
        let Some(bound_state) = inner.runtime_state.as_ref() else {
            drop(inner);
            self.bind_runtime_state(state);
            return true;
        };
        if Rc::ptr_eq(bound_state, state) {
            return true;
        }
        if bound_state.has_live_applier_host() {
            return false;
        }
        drop(inner);

        let mut inner = self.inner.borrow_mut();
        let Some(bound_state) = inner.runtime_state.as_ref() else {
            inner.runtime_state = Some(Rc::clone(state));
            return true;
        };
        if Rc::ptr_eq(bound_state, state) {
            return true;
        }
        if bound_state.has_live_applier_host() {
            return false;
        }

        let previous_state = Rc::clone(bound_state);
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        lifecycle.flush_pending_drops();
        let host_key = self.storage_key();
        if previous_state
            .dispose_retained_subtrees_for_host(host_key, &mut inner.table, &mut lifecycle)
            .is_err()
        {
            inner.lifecycle = lifecycle;
            return false;
        }
        previous_state.clear_host(self, &mut inner.table);
        lifecycle.flush_pending_drops();
        inner.runtime_state = Some(Rc::clone(state));
        inner.lifecycle = lifecycle;
        true
    }

    pub(crate) fn runtime_state(&self) -> Option<Rc<composer::ComposerRuntimeState>> {
        self.inner.borrow().runtime_state.clone()
    }

    pub(crate) fn runtime_state_ptr(&self) -> Option<*const composer::ComposerRuntimeState> {
        self.inner.borrow().runtime_state.as_ref().map(Rc::as_ptr)
    }

    pub(crate) fn borrow(&self) -> Ref<'_, SlotTable> {
        Ref::map(self.inner.borrow(), |inner| &inner.table)
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, SlotTable> {
        RefMut::map(self.inner.borrow_mut(), |inner| &mut inner.table)
    }

    /// Takes the slot table out of the host after disposing what it retained. Fails if other
    /// references to the host exist or a slot pass is active.
    pub fn into_table(self: Rc<Self>) -> Result<SlotTable, NodeError> {
        if Rc::strong_count(&self) != 1 {
            return Err(NodeError::SlotHostUnavailable {
                operation: "SlotsHost::into_table",
                reason: "other host references are alive",
            });
        }
        self.take_table_for_transfer()
    }

    fn take_table_for_transfer(&self) -> Result<SlotTable, NodeError> {
        let inner = self.inner.borrow();
        if inner.active_pass.is_some() {
            return Err(NodeError::SlotHostUnavailable {
                operation: "SlotsHost::into_table",
                reason: "slot pass is active",
            });
        }
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        lifecycle.flush_pending_drops();
        if let Some(state) = inner.runtime_state.clone() {
            let host_key = self.storage_key();
            if let Err(error) =
                state.dispose_retained_subtrees_for_host(host_key, &mut inner.table, &mut lifecycle)
            {
                inner.lifecycle = lifecycle;
                return Err(error);
            }
            state.clear_host(self, &mut inner.table);
            lifecycle.flush_pending_drops();
        }
        let taken = std::mem::take(&mut inner.table);
        self.storage_key.set(inner.table.storage_id());
        inner.runtime_state = None;
        inner.lifecycle = lifecycle;
        Ok(taken)
    }

    /// Disposes the content of the slot table and replaces it with an empty one. Fails while a slot
    /// pass is active.
    pub fn reset(&self) -> Result<(), NodeError> {
        let inner = self.inner.borrow();
        if inner.active_pass.is_some() {
            return Err(NodeError::SlotHostUnavailable {
                operation: "SlotsHost::reset",
                reason: "slot pass is active",
            });
        }
        let runtime_state = inner.runtime_state.clone();
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        if let Some(state) = runtime_state {
            let host_key = self.storage_key();
            if let Err(error) =
                state.dispose_retained_subtrees_for_host(host_key, &mut inner.table, &mut lifecycle)
            {
                inner.lifecycle = lifecycle;
                return Err(error);
            }
            state.clear_host(self, &mut inner.table);
        }
        lifecycle.dispose_slot_table(&mut inner.table);
        inner.table = SlotTable::default();
        self.storage_key.set(inner.table.storage_id());
        inner.runtime_state = None;
        inner.lifecycle = slot::SlotLifecycleCoordinator::default();
        Ok(())
    }

    pub(crate) fn abandon_after_apply_failure(&self) {
        let inner = self.inner.borrow();
        if inner.active_pass.is_some() {
            log::error!("cannot abandon SlotsHost during an active pass");
            return;
        }
        let runtime_state = inner.runtime_state.clone();
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        if let Some(state) = runtime_state {
            let host_key = self.storage_key();
            state.abandon_retained_subtrees_for_host(host_key, &mut inner.table, &mut lifecycle);
        }
        lifecycle.dispose_slot_table(&mut inner.table);
        inner.table = SlotTable::default();
        self.storage_key.set(inner.table.storage_id());
        inner.runtime_state = None;
        inner.lifecycle = slot::SlotLifecycleCoordinator::default();
    }

    pub(crate) fn debug_stats(&self) -> SlotTableDebugStats {
        let inner = self.inner.borrow();
        let local = inner.table.debug_stats();
        let lifecycle = inner.lifecycle.debug_stats();
        let retention = inner
            .runtime_state
            .clone()
            .map(|state| state.slot_retention_debug_stats(self))
            .unwrap_or_default();
        SlotTableDebugStats::from_parts(local, lifecycle, retention)
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn debug_snapshot(&self) -> SlotDebugSnapshot {
        let inner = self.inner.borrow();
        let mut snapshot = inner.table.debug_snapshot();
        if let Some(state) = inner.runtime_state.clone() {
            state.fill_slot_debug_snapshot(self, &mut snapshot);
        }
        snapshot
    }

    pub(crate) fn begin_pass(&self, mode: slot::SlotPassMode, folds: &Rc<slot::BranchFolds>) {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            active_pass,
            pass_state,
            ..
        } = &mut *inner;
        if active_pass.is_some() {
            log::error!("slot pass already active for host");
            return;
        }
        let state = match pass_state {
            Some(state) => {
                state.set_folds(Rc::clone(folds));
                state
            }
            None => pass_state.insert(slot::SlotWriteSessionState::new(Rc::clone(folds))),
        };
        state.reset_for_pass(mode);
        *active_pass = Some(table.storage_capacity());
    }

    pub(crate) fn has_active_pass(&self) -> bool {
        self.inner.borrow().active_pass.is_some()
    }

    pub(crate) fn abandon_active_pass(&self) {
        self.inner.borrow_mut().active_pass = None;
    }

    pub(crate) fn with_write_session<R>(
        &self,
        f: impl FnOnce(&mut slot::SlotWriteSession<'_>) -> R,
    ) -> R {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            lifecycle,
            active_pass,
            pass_state,
            ..
        } = &mut *inner;
        let state = pass_state
            .as_mut()
            .filter(|_| active_pass.is_some())
            .expect("slot write session requires an active pass");
        let mut session = table.write_session(lifecycle, state);
        f(&mut session)
    }

    pub(crate) fn with_table_and_lifecycle_mut<R>(
        &self,
        f: impl FnOnce(&mut SlotTable, &mut slot::SlotLifecycleCoordinator) -> R,
    ) -> R {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table, lifecycle, ..
        } = &mut *inner;
        f(table, lifecycle)
    }

    pub(crate) fn finish_pass(
        &self,
        applier: &mut dyn Applier,
    ) -> Result<FinishedSlotPass, NodeError> {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            lifecycle,
            active_pass,
            pass_state,
            ..
        } = &mut *inner;
        let (Some(storage_capacity), Some(state)) = (active_pass.take(), pass_state.as_mut())
        else {
            return Ok(FinishedSlotPass::default());
        };

        state.flush_payload_location_refreshes(table);

        #[cfg(any(test, debug_assertions))]
        if let Err(err) = state.validate(table) {
            log::error!("slot writer invariant violation before finalize_pass: {err:?}");
            return Err(NodeError::SlotHostUnavailable {
                operation: "SlotsHost::finish_pass",
                reason: "slot writer invariant violation",
            });
        }

        let detached_root_children = {
            let mut session = table.write_session(lifecycle, state);
            session.finalize_pass(applier)?
        };

        let outcome = SlotPassOutcome {
            compacted: state.request_compaction,
            compact_anchor_registry_storage: state.request_anchor_storage_compaction,
            compact_payload_storage: state.request_payload_storage_compaction,
            trim_growth_slack: state.removed_nothing()
                && table.storage_capacity() > storage_capacity,
        };
        Ok(FinishedSlotPass {
            outcome,
            detached_root_children,
        })
    }

    pub(crate) fn flush_pending_drops(&self) {
        self.inner.borrow_mut().lifecycle.flush_pending_drops();
    }

    pub(crate) fn complete_pass_cleanup(&self, outcome: &SlotPassOutcome) {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            lifecycle,
            runtime_state,
            ..
        } = &mut *inner;
        lifecycle.flush_pending_drops();
        if outcome.compacted {
            table.compact_storage();
            lifecycle.compact_storage();
        } else if outcome.trim_growth_slack {
            table.trim_growth_slack();
        }
        if let Some(state) = runtime_state.clone() {
            state.compact_table_identity_storage_for_host(
                self,
                table,
                outcome.compact_anchor_registry_storage,
                outcome.compact_payload_storage,
            );
        } else {
            if outcome.compact_anchor_registry_storage {
                table.compact_anchor_registry_storage(None);
            }
            if outcome.compact_payload_storage {
                table.compact_payload_anchor_registry_storage(None);
            }
        }
        table.assert_fast_integrity("slot pass cleanup");
        #[cfg(any(test, debug_assertions))]
        {
            table.debug_verify();
            if let Some(state) = runtime_state.clone() {
                state.debug_verify_host(self, table);
            }
        }
    }
}

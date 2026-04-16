use crate::{
    collections::map::HashMap, remove_child_and_cleanup_now, runtime, snapshot_state_observer,
    Applier, ApplierGuard, ApplierHost, CommandQueue, Composer, CompositionPassDebugStats,
    ConcreteApplierHost, DefaultScheduler, Key, NodeError, NodeId, RecomposeScope, Runtime,
    RuntimeHandle, ScopeId, SlotTable, SlotTableDebugStats, SlotValueTypeDebugStat, SlotsHost,
    SnapshotStateObserver,
};
use std::rc::Rc;
use std::sync::Arc;

pub struct Composition<A: Applier + 'static> {
    pub(crate) slots: Rc<SlotsHost>,
    pub(crate) applier: Rc<ConcreteApplierHost<A>>,
    pub(crate) runtime: Runtime,
    pub(crate) observer: SnapshotStateObserver,
    pub(crate) root: Option<NodeId>,
    pub(crate) root_key: Option<Key>,
    pub(crate) root_render_requested: bool,
    pub(crate) last_pass_stats: CompositionPassDebugStats,
}

/// Upper bound on chained root-render replays and scope-recomposition rounds.
///
/// Each root render clears `root_render_requested` but may re-raise it if a
/// recompose pass inside `render()` promotes a scope callback to the root
/// (see `Composer::recranpose_group` in recompose.rs — callbacks that cannot
/// run invalidate their `callback_promotion_target`, and if no ancestor can
/// absorb the callback, `request_root_render()` is called). Each promotion
/// walks up one parent scope, so natural convergence is bounded by the
/// composition depth. The same invariant bounds `process_invalid_scopes`:
/// recomposing a scope may invalidate others, but the chain must terminate.
///
/// This constant is a safety net for reentrant-render bugs. Exceeding it
/// trips a `debug_assert!` in dev/test builds (loud failure so regressions
/// are caught immediately) and falls back to a break + `log::error!` in
/// release builds so end users do not see the UI thread panic.
pub const ROOT_RENDER_REPLAY_LIMIT: usize = 100;

impl<A: Applier + 'static> Composition<A> {
    pub fn new(applier: A) -> Self {
        Self::with_runtime(applier, Runtime::new(Arc::new(DefaultScheduler)))
    }

    pub fn with_runtime(applier: A, runtime: Runtime) -> Self {
        let slots = Rc::new(SlotsHost::new(SlotTable::new()));
        let applier = Rc::new(ConcreteApplierHost::new(applier));
        let observer_handle = runtime.handle();
        let observer = SnapshotStateObserver::new(move |callback| {
            observer_handle.enqueue_ui_task(callback);
        });
        observer.start();
        Self {
            slots,
            applier,
            runtime,
            observer,
            root: None,
            root_key: None,
            root_render_requested: false,
            last_pass_stats: CompositionPassDebugStats::default(),
        }
    }

    /// Returns the root group key captured from the most recent `render()` call,
    /// or `None` before the first render.
    pub fn root_key(&self) -> Option<Key> {
        self.root_key
    }

    fn slots_host(&self) -> Rc<SlotsHost> {
        Rc::clone(&self.slots)
    }

    fn applier_host(&self) -> Rc<dyn ApplierHost> {
        self.applier.clone()
    }

    fn reset_last_pass_stats(&mut self) {
        self.last_pass_stats = CompositionPassDebugStats::default();
    }

    pub fn take_root_render_request(&mut self) -> bool {
        std::mem::take(&mut self.root_render_requested)
    }

    fn record_pass_stats(
        &mut self,
        commands: &CommandQueue,
        side_effects: &Vec<Box<dyn FnOnce()>>,
    ) {
        self.last_pass_stats.commands_len = self.last_pass_stats.commands_len.max(commands.len());
        self.last_pass_stats.commands_cap =
            self.last_pass_stats.commands_cap.max(commands.capacity());
        self.last_pass_stats.command_payload_len_bytes = self
            .last_pass_stats
            .command_payload_len_bytes
            .max(commands.payload_len_bytes());
        self.last_pass_stats.command_payload_cap_bytes = self
            .last_pass_stats
            .command_payload_cap_bytes
            .max(commands.payload_capacity_bytes());
        self.last_pass_stats.sync_children_len = self
            .last_pass_stats
            .sync_children_len
            .max(commands.sync_children.len());
        self.last_pass_stats.sync_children_cap = self
            .last_pass_stats
            .sync_children_cap
            .max(commands.sync_children.capacity());
        self.last_pass_stats.sync_child_ids_len = self
            .last_pass_stats
            .sync_child_ids_len
            .max(commands.sync_child_ids.len());
        self.last_pass_stats.sync_child_ids_cap = self
            .last_pass_stats
            .sync_child_ids_cap
            .max(commands.sync_child_ids.capacity());
        self.last_pass_stats.side_effects_len = self
            .last_pass_stats
            .side_effects_len
            .max(side_effects.len());
        self.last_pass_stats.side_effects_cap = self
            .last_pass_stats
            .side_effects_cap
            .max(side_effects.capacity());
    }

    pub(crate) fn finalize_compaction(&mut self) -> Result<bool, NodeError> {
        let mut removed_orphaned = false;
        let mut orphaned_node_count = 0usize;
        self.slots.borrow_mut().compact();
        let orphaned = self.slots.borrow_mut().drain_orphaned_node_ids();
        {
            let mut applier = self.applier.borrow_dyn();
            for orphaned in orphaned {
                if !matches!(
                    self.slots.borrow().orphaned_node_state(orphaned),
                    crate::slot_table::NodeSlotState::Missing
                ) {
                    continue;
                }
                if applier.node_generation(orphaned.id) != orphaned.generation {
                    continue;
                }
                removed_orphaned = true;
                orphaned_node_count += 1;
                let parent_id = applier
                    .get_mut(orphaned.id)
                    .ok()
                    .and_then(|node| node.parent());
                if let Some(parent_id) = parent_id {
                    let _ = remove_child_and_cleanup_now(&mut *applier, parent_id, orphaned.id);
                    continue;
                }
                if let Ok(node) = applier.get_mut(orphaned.id) {
                    node.on_removed_from_parent();
                    node.unmount();
                }
                let _ = applier.remove(orphaned.id);
            }
        }
        self.applier_host().compact();
        self.applier.borrow_dyn().clear_recycled_nodes();
        if removed_orphaned {
            log::debug!(
                "finalize_compaction: removing {} orphaned nodes",
                orphaned_node_count
            );
        }
        Ok(removed_orphaned)
    }

    pub fn render(&mut self, key: Key, mut content: impl FnMut()) -> Result<(), NodeError> {
        self.reset_last_pass_stats();
        self.root_key = Some(key);
        self.root_render_requested = false;
        self.slots.borrow_mut().reset();
        let runtime_handle = self.runtime_handle();
        runtime_handle.drain_ui();
        let side_effects = {
            let _teardown = runtime::enter_state_teardown_scope();
            let composer = Composer::new(
                Rc::clone(&self.slots),
                self.applier.clone(),
                runtime_handle.clone(),
                self.observer.clone(),
                self.root,
            );
            self.observer.begin_frame();
            let (root, commands, side_effects) = composer.install(|composer| {
                composer.with_group(key, |_| content());
                let root = composer.root();
                let commands = composer.take_commands();
                let side_effects = composer.take_side_effects();
                (root, commands, side_effects)
            });
            self.record_pass_stats(&commands, &side_effects);
            {
                let mut applier = self.applier.borrow_dyn();
                commands.apply(&mut *applier)?;
                for update in runtime_handle.take_updates() {
                    update.apply(&mut *applier)?;
                }
            }

            self.root = root;
            {
                let mut slots = self.slots.borrow_mut();
                let _ = slots.finalize_current_group();
                slots.flush();
            }
            let _ = self.finalize_compaction()?;
            side_effects
        };
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        let _ = self.process_invalid_scopes()?;
        self.observer.prune_dead_scopes();
        if !self.runtime.has_updates()
            && !runtime_handle.has_invalid_scopes()
            && !runtime_handle.has_frame_callbacks()
            && !runtime_handle.has_pending_ui()
        {
            self.runtime.set_needs_frame(false);
        }
        Ok(())
    }

    /// Returns true if composition needs to process invalid scopes (recompose).
    ///
    /// This checks both:
    /// - `has_updates()`: composition scopes that were invalidated by state changes
    /// - `needs_frame()`: animation callbacks that may have pending work
    ///
    /// Note: For scroll performance, ensure scroll state changes use Cell<T> instead
    /// of MutableState<T> to avoid triggering recomposition on every scroll frame.
    pub fn should_render(&self) -> bool {
        self.root_render_requested || self.runtime.needs_frame() || self.runtime.has_updates()
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }

    pub fn applier_mut(&mut self) -> ApplierGuard<'_, A> {
        ApplierGuard::new(self.applier.borrow_typed())
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn debug_dump_slot_table_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.slots.borrow().debug_dump_groups()
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        self.slots.borrow().debug_dump_all_slots()
    }

    pub fn slot_table_heap_bytes(&self) -> usize {
        self.slots.borrow().heap_bytes()
    }

    pub fn debug_slot_table_stats(&self) -> SlotTableDebugStats {
        self.slots.borrow().debug_stats()
    }

    pub fn debug_slot_value_type_counts(&self, limit: usize) -> Vec<SlotValueTypeDebugStat> {
        self.slots.borrow().debug_value_type_counts(limit)
    }

    pub fn debug_observer_stats(&self) -> snapshot_state_observer::SnapshotStateObserverDebugStats {
        self.observer.debug_stats()
    }

    pub fn debug_last_pass_stats(&self) -> CompositionPassDebugStats {
        self.last_pass_stats
    }

    pub fn process_invalid_scopes(&mut self) -> Result<bool, NodeError> {
        let runtime_handle = self.runtime_handle();
        let mut did_recompose = false;
        let mut loop_count = 0;
        loop {
            loop_count += 1;
            if loop_count > ROOT_RENDER_REPLAY_LIMIT {
                debug_assert!(
                    false,
                    "process_invalid_scopes exceeded {ROOT_RENDER_REPLAY_LIMIT} iterations — reentrant recomposition bug (a scope keeps re-invalidating)"
                );
                log::error!(
                    "process_invalid_scopes looped past {ROOT_RENDER_REPLAY_LIMIT} iterations; breaking to keep UI responsive"
                );
                break;
            }
            runtime_handle.drain_ui();
            let pending = runtime_handle.take_invalidated_scopes();
            if pending.is_empty() {
                break;
            }
            let mut scopes = Vec::new();
            for (id, weak) in pending {
                if let Some(inner) = weak.upgrade() {
                    scopes.push(RecomposeScope { inner });
                } else {
                    runtime_handle.mark_scope_recomposed(id);
                }
            }
            if scopes.is_empty() {
                continue;
            }
            did_recompose = true;
            let runtime_clone = runtime_handle.clone();
            let root_host = self.slots_host();
            let mut scope_groups: Vec<(Rc<SlotsHost>, Vec<RecomposeScope>)> = Vec::new();
            let mut scope_group_index: HashMap<usize, usize> = HashMap::default();
            for scope in scopes {
                let host = scope.slots_host().unwrap_or_else(|| Rc::clone(&root_host));
                let host_key = Rc::as_ptr(&host) as usize;
                if let Some(index) = scope_group_index.get(&host_key).copied() {
                    scope_groups[index].1.push(scope);
                } else {
                    scope_group_index.insert(host_key, scope_groups.len());
                    scope_groups.push((host, vec![scope]));
                }
            }
            let side_effects = {
                let _teardown = runtime::enter_state_teardown_scope();
                let composer = Composer::new(
                    Rc::clone(&root_host),
                    self.applier_host(),
                    runtime_clone,
                    self.observer.clone(),
                    self.root,
                );
                self.observer.begin_frame();
                let (root, commands, side_effects, requested_root_render) =
                    composer.install(|composer| {
                        for (host, scopes) in scope_groups.into_iter() {
                            if Rc::ptr_eq(&host, &root_host) {
                                for scope in &scopes {
                                    composer.recranpose_group(scope);
                                }
                            } else {
                                composer.with_slot_override(host, |composer| {
                                    for scope in &scopes {
                                        composer.recranpose_group(scope);
                                    }
                                });
                            }
                        }
                        let root = composer.root();
                        let commands = composer.take_commands();
                        let side_effects = composer.take_side_effects();
                        let requested_root_render = composer.take_root_render_request();
                        (root, commands, side_effects, requested_root_render)
                    });
                self.record_pass_stats(&commands, &side_effects);
                {
                    let mut applier = self.applier.borrow_dyn();
                    commands.apply(&mut *applier)?;
                    for update in runtime_handle.take_updates() {
                        update.apply(&mut *applier)?;
                    }
                }
                if root.is_some() {
                    self.root = root;
                }
                {
                    let mut slots = self.slots.borrow_mut();
                    slots.flush();
                }
                let removed_orphaned = self.finalize_compaction()?;
                if removed_orphaned {
                    did_recompose = true;
                    self.root_render_requested = true;
                }
                if requested_root_render {
                    self.root_render_requested = true;
                }
                side_effects
            };
            runtime_handle.drain_ui();
            for effect in side_effects {
                effect();
            }
            runtime_handle.drain_ui();
            if self.root_render_requested {
                break;
            }
        }
        self.observer.prune_dead_scopes();
        if !self.runtime.has_updates()
            && !runtime_handle.has_invalid_scopes()
            && !runtime_handle.has_frame_callbacks()
            && !runtime_handle.has_pending_ui()
        {
            self.runtime.set_needs_frame(false);
        }
        Ok(did_recompose)
    }

    pub fn flush_pending_node_updates(&mut self) -> Result<(), NodeError> {
        let updates = self.runtime_handle().take_updates();
        let mut applier = self.applier.borrow_dyn();
        for update in updates {
            update.apply(&mut *applier)?;
        }
        Ok(())
    }
}

impl<A: Applier + 'static> Drop for Composition<A> {
    fn drop(&mut self) {
        self.observer.stop();
    }
}

use std::rc::Rc;

use web_time::Instant;

use crate::{
    Applier, ApplierGuard, ApplierHost, CommandQueue, Composer, CompositionPassDebugStats,
    ConcreteApplierHost, DefaultScheduler, Key, NodeError, NodeId, RecomposeScope, RetentionPolicy,
    Runtime, RuntimeHandle, ScopeId, SlotDebugSnapshot, SlotTable, SlotTableDebugStats, SlotsHost,
    SnapshotStateObserver, collections::map::HashMap, debug_scope_invalidation_sources,
    debug_scope_label, runtime, scheduler_ref, snapshot_state_observer,
};

pub struct Composition<A: Applier + 'static> {
    pub(crate) composer_state: Rc<crate::composer::ComposerRuntimeState>,
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
/// (see `Composer::recompose_group` in recompose.rs — callbacks that cannot
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

fn recompose_scope_telemetry_threshold_ms() -> Option<f64> {
    std::env::var("CRANPOSE_RECOMPOSE_SCOPE_TELEMETRY_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
}

impl<A: Applier + 'static> Composition<A> {
    pub fn new(applier: A) -> Self {
        Self::with_runtime(applier, Runtime::new(scheduler_ref(DefaultScheduler)))
    }

    pub fn with_runtime(applier: A, runtime: Runtime) -> Self {
        let composer_state = Rc::new(crate::composer::ComposerRuntimeState::default());
        let slots = Rc::new(SlotsHost::new(SlotTable::new()));
        let applier = Rc::new(ConcreteApplierHost::new(applier));
        let observer_handle = runtime.handle();
        let observer = SnapshotStateObserver::new(move |callback| {
            observer_handle.enqueue_ui_task(callback);
        });
        observer.start();
        Self {
            composer_state,
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

    pub fn set_retention_policy(&self, policy: RetentionPolicy) {
        self.composer_state.set_retention_policy(policy);
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

    fn maybe_dump_slot_table(&self, label: &str) {
        if !crate::env_flag!("COMPOSE_DEBUG_SLOT_TABLE") {
            return;
        }
        eprintln!(
            "[COMPOSE_DEBUG_SLOT_TABLE] {label}\n{:#?}",
            self.debug_slot_snapshot()
        );
    }

    pub fn take_root_render_request(&mut self) -> bool {
        std::mem::take(&mut self.root_render_requested)
    }

    pub fn request_root_render(&mut self) {
        self.root_render_requested = true;
        self.runtime.handle().schedule();
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

    fn finalize_runtime_state(&mut self) {
        let runtime_handle = self.runtime_handle();
        self.observer.prune_dead_scopes();
        if !self.runtime.has_updates()
            && !runtime_handle.has_invalid_scopes()
            && !runtime_handle.has_frame_callbacks()
            && !runtime_handle.has_pending_ui()
        {
            self.runtime.set_needs_frame(false);
        }
    }

    fn abandon_host_after_apply_failure(&mut self, host: &Rc<SlotsHost>) {
        host.abandon_after_apply_failure();
        if Rc::ptr_eq(host, &self.slots) {
            self.root = None;
        }
        self.root_render_requested = true;
        self.finalize_runtime_state();
    }

    fn apply_commands_and_updates_for_host(
        &mut self,
        host: &Rc<SlotsHost>,
        runtime_handle: &RuntimeHandle,
        commands: CommandQueue,
    ) -> Result<(), NodeError> {
        let result = {
            let mut applier = self.applier.borrow_dyn();
            let mut result = commands.apply(&mut *applier);
            if result.is_ok() {
                for update in runtime_handle.take_updates() {
                    if let Err(err) = update.apply(&mut *applier) {
                        result = Err(err);
                        break;
                    }
                }
            }
            result
        };
        if result.is_err() {
            self.abandon_host_after_apply_failure(host);
        }
        result
    }

    fn render_root_pass(&mut self, key: Key, content: &mut dyn FnMut()) -> Result<(), NodeError> {
        self.root_key = Some(key);
        self.root_render_requested = false;
        let runtime_handle = self.runtime_handle();
        runtime_handle.drain_ui();
        let side_effects = {
            let _teardown = runtime::enter_state_teardown_scope();
            let composer = Composer::new_with_shared_state(
                Rc::clone(&self.composer_state),
                Rc::clone(&self.slots),
                self.applier.clone(),
                runtime_handle.clone(),
                self.observer.clone(),
                self.root,
            );
            self.observer.begin_frame();
            let (root, commands, side_effects, compact_applier) = composer.install(|composer| {
                let (_, outcome) = composer.try_with_slot_host_pass(
                    Rc::clone(&self.slots),
                    crate::slot::SlotPassMode::Compose,
                    |composer| composer.with_group(key, |_| content()),
                )?;
                let root = composer.root();
                let commands = composer.take_commands();
                let side_effects = composer.take_side_effects();
                Ok((root, commands, side_effects, outcome.compacted))
            })?;
            self.record_pass_stats(&commands, &side_effects);
            self.apply_commands_and_updates_for_host(
                &Rc::clone(&self.slots),
                &runtime_handle,
                commands,
            )?;
            if compact_applier {
                self.applier.compact();
                self.applier.borrow_dyn().clear_recycled_nodes();
            }

            self.root = root;
            side_effects
        };
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        self.maybe_dump_slot_table("root_render_pass");
        Ok(())
    }

    fn reconcile_with_content(
        &mut self,
        key: Key,
        content: &mut dyn FnMut(),
    ) -> Result<bool, NodeError> {
        self.root_key = Some(key);
        let mut did_work = false;
        let mut root_render_replays = 0usize;
        loop {
            did_work |= self.process_invalid_scopes_until_root_request()?;
            if !self.take_root_render_request() {
                return Ok(did_work);
            }

            root_render_replays += 1;
            if root_render_replays > ROOT_RENDER_REPLAY_LIMIT {
                log::error!(
                    "root render replay looped past {ROOT_RENDER_REPLAY_LIMIT} iterations; breaking to keep UI responsive"
                );
                return Err(NodeError::RecompositionLimitExceeded {
                    operation: "root render replay",
                    limit: ROOT_RENDER_REPLAY_LIMIT,
                });
            }

            self.render_root_pass(key, content)?;
            did_work = true;
        }
    }

    pub fn render(&mut self, key: Key, mut content: impl FnMut()) -> Result<(), NodeError> {
        self.reset_last_pass_stats();
        self.render_root_pass(key, &mut content)?;
        let _ = self.process_invalid_scopes()?;
        Ok(())
    }

    /// Perform a root render and continue replaying any resulting root-render
    /// requests until the composition reaches a stable fixpoint.
    pub fn render_stable(&mut self, key: Key, mut content: impl FnMut()) -> Result<(), NodeError> {
        self.reset_last_pass_stats();
        self.render_root_pass(key, &mut content)?;
        let _ = self.reconcile_with_content(key, &mut content)?;
        Ok(())
    }

    /// Process invalid scopes and any resulting root-render requests until the
    /// composition reaches a stable fixpoint for the supplied root content.
    pub fn reconcile(&mut self, key: Key, mut content: impl FnMut()) -> Result<bool, NodeError> {
        self.reconcile_with_content(key, &mut content)
    }

    /// Returns true if composition needs to process invalid scopes (recompose).
    ///
    /// This checks both:
    /// - `has_updates()`: composition scopes that were invalidated by state changes
    /// - `needs_frame()`: animation callbacks that may have pending work
    ///
    /// Note: For scroll performance, ensure scroll state changes use `Cell<T>` instead
    /// of `MutableState<T>` to avoid triggering recomposition on every scroll frame.
    pub fn should_render(&self) -> bool {
        self.root_render_requested || self.runtime.needs_frame() || self.runtime.has_updates()
    }

    /// Whether any composition scope is actually invalid, i.e. whether running
    /// the composable tree could produce a different result than last time.
    ///
    /// Deliberately excludes [`Runtime::needs_frame`], which [`Self::should_render`]
    /// includes. An armed frame callback means the *runtime* owes someone a
    /// tick - a future to resume, a dispatcher to drain - and every app with a
    /// game loop or a polling effect has one armed at all times. Re-running the
    /// composition for it recomposes a tree that nothing invalidated. Callers
    /// deciding "should I tick" want [`Self::should_render`]; callers deciding
    /// "should I recompose" want this.
    ///
    /// It is only meaningful *after* the frame callbacks have been drained: a
    /// callback that writes state invalidates its readers as it runs, so the
    /// answer is a question about work already discovered, not work still to
    /// come.
    pub fn should_recompose(&self) -> bool {
        self.root_render_requested || self.runtime.has_updates()
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

    pub fn debug_dump_slot_entries(&self) -> Vec<crate::SlotDebugEntry> {
        self.slots.borrow().debug_dump_slot_entries()
    }

    pub fn slot_table_heap_bytes(&self) -> usize {
        self.slots.borrow().heap_bytes()
    }

    pub fn debug_slot_table_stats(&self) -> SlotTableDebugStats {
        self.slots.debug_stats()
    }

    pub fn debug_slot_snapshot(&self) -> SlotDebugSnapshot {
        self.slots.debug_snapshot()
    }

    pub fn debug_observer_stats(&self) -> snapshot_state_observer::SnapshotStateObserverDebugStats {
        self.observer.debug_stats()
    }

    pub fn debug_last_pass_stats(&self) -> CompositionPassDebugStats {
        self.last_pass_stats
    }

    #[cfg(test)]
    pub(crate) fn debug_validate_slots(&self) -> Result<(), crate::slot::SlotInvariantError> {
        let table = self.slots.borrow();
        table.validate()?;
        self.composer_state
            .validate_host_retention(self.slots.as_ref(), &table)
    }

    fn process_invalid_scopes_until_root_request(&mut self) -> Result<bool, NodeError> {
        let runtime_handle = self.runtime_handle();
        let mut did_recompose = false;
        let mut loop_count = 0;
        loop {
            loop_count += 1;
            if loop_count > ROOT_RENDER_REPLAY_LIMIT {
                log::error!(
                    "process_invalid_scopes looped past {ROOT_RENDER_REPLAY_LIMIT} iterations; breaking to keep UI responsive"
                );
                return Err(NodeError::RecompositionLimitExceeded {
                    operation: "process_invalid_scopes",
                    limit: ROOT_RENDER_REPLAY_LIMIT,
                });
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
                let host = scope
                    .slots_runtime_state()
                    .and_then(|state| {
                        scope
                            .slots_storage_key()
                            .and_then(|storage_key| state.host_for_storage_key(storage_key))
                    })
                    .or_else(|| {
                        scope.slots_storage_key().and_then(|storage_key| {
                            self.composer_state.host_for_storage_key(storage_key)
                        })
                    })
                    .unwrap_or_else(|| Rc::clone(&root_host));
                let host_key = host.storage_key();
                if let Some(index) = scope_group_index.get(&host_key).copied() {
                    scope_groups[index].1.push(scope);
                } else {
                    scope_group_index.insert(host_key, scope_groups.len());
                    scope_groups.push((host, vec![scope]));
                }
            }
            let mut host_group_index = 0usize;
            while host_group_index < scope_groups.len() {
                let (host, scopes) = &scope_groups[host_group_index];
                let scope_telemetry_threshold_ms = recompose_scope_telemetry_threshold_ms();
                let shared_state = host
                    .runtime_state()
                    .or_else(|| scopes.first().and_then(RecomposeScope::slots_runtime_state))
                    .unwrap_or_else(|| Rc::clone(&self.composer_state));
                let side_effects = {
                    let _teardown = runtime::enter_state_teardown_scope();
                    let composer = Composer::new_with_shared_state(
                        shared_state,
                        Rc::clone(host),
                        self.applier_host(),
                        runtime_clone.clone(),
                        self.observer.clone(),
                        self.root,
                    );
                    composer.parent_stack().clear();
                    self.observer.begin_frame();
                    let (root, commands, side_effects, requested_root_render, compact_applier) =
                        composer.install(|composer| {
                            let (_, outcome) = composer.try_with_slot_host_pass(
                                Rc::clone(host),
                                crate::slot::SlotPassMode::Recompose,
                                |composer| {
                                    for scope in scopes {
                                        if let Some(threshold_ms) = scope_telemetry_threshold_ms {
                                            let start = Instant::now();
                                            composer.recompose_group(scope);
                                            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                                            if elapsed_ms >= threshold_ms {
                                                eprintln!(
                                                    "[recompose-scope-telemetry] scope_id={} label={:?} elapsed_ms={elapsed_ms:.3} invalidation_sources={:?}",
                                                    scope.id(),
                                                    debug_scope_label(scope.id()),
                                                    debug_scope_invalidation_sources(scope.id())
                                                );
                                            }
                                        } else {
                                            composer.recompose_group(scope);
                                        }
                                    }
                                },
                            )?;
                            let root = composer.root();
                            let commands = composer.take_commands();
                            let side_effects = composer.take_side_effects();
                            let requested_root_render = composer.take_root_render_request();
                            Ok((
                                root,
                                commands,
                                side_effects,
                                requested_root_render,
                                outcome.compacted,
                            ))
                        })?;
                    self.record_pass_stats(&commands, &side_effects);
                    self.apply_commands_and_updates_for_host(host, &runtime_handle, commands)?;
                    if compact_applier {
                        self.applier.compact();
                        self.applier.borrow_dyn().clear_recycled_nodes();
                    }
                    if root.is_some() {
                        self.root = root;
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
                self.maybe_dump_slot_table("recompose_pass");
                if self.root_render_requested {
                    for (_, remaining_scopes) in scope_groups.iter().skip(host_group_index + 1) {
                        for scope in remaining_scopes {
                            runtime_handle.requeue_invalid_scope(scope.id(), scope.downgrade());
                        }
                    }
                    break;
                }
                host_group_index += 1;
            }
            if self.root_render_requested {
                break;
            }
        }
        self.finalize_runtime_state();
        Ok(did_recompose)
    }

    pub fn process_invalid_scopes(&mut self) -> Result<bool, NodeError> {
        self.process_invalid_scopes_until_root_request()
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

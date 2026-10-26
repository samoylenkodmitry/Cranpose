use std::rc::Rc;

use smallvec::SmallVec;
use web_time::Instant;

use crate::{
    Applier, ApplierGuard, ApplierHost, CommandQueue, Composer, ConcreteApplierHost, DIAGNOSTICS,
    DefaultScheduler, Key, NodeError, NodeId, RecomposeScope, RetentionPolicy, Runtime,
    RuntimeHandle, ScopeId, SlotDebugSnapshot, SlotTable, SlotTableDebugStats, SlotsHost,
    collections::map::HashMap, debug_scope_invalidation_sources, debug_scope_label, runtime,
    scheduler_ref,
};

/// A running composition that turns root content into nodes. `A` is the applier type that creates
/// and edits those nodes.
pub struct Composition<A: Applier + 'static> {
    /// The composer of the last pass over the root slots, kept so the next
    /// pass reuses its core. First, so it lets go of the tree before the
    /// fields below do.
    parked_composer: Option<Composer>,
    pub(crate) composer_state: Rc<crate::composer::ComposerRuntimeState>,
    pub(crate) slots: Rc<SlotsHost>,
    pub(crate) applier: Rc<ConcreteApplierHost<A>>,
    pub(crate) runtime: Runtime,
    pub(crate) root: Option<NodeId>,
    pub(crate) root_key: Option<Key>,
    pub(crate) root_render_requested: bool,
    pub(crate) last_pass_stats: CompositionPassDebugStats,
    teardown: Option<runtime::StateTeardownScope>,
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

impl<A: Applier + 'static> Composition<A> {
    /// Creates a composition that builds its nodes through `applier` and uses a new runtime with
    /// the default scheduler.
    pub fn new(applier: A) -> Self {
        Self::with_runtime(applier, Runtime::new(scheduler_ref(DefaultScheduler)))
    }

    /// Creates a composition that builds its nodes through `applier` and uses `runtime` for state
    /// invalidation and scheduling.
    pub fn with_runtime(applier: A, runtime: Runtime) -> Self {
        let composer_state = Rc::new(crate::composer::ComposerRuntimeState::default());
        let slots = Rc::new(SlotsHost::new(SlotTable::new()));
        let applier = Rc::new(ConcreteApplierHost::new(applier));
        Self {
            parked_composer: None,
            composer_state,
            slots,
            applier,
            runtime,
            root: None,
            root_key: None,
            root_render_requested: false,
            last_pass_stats: CompositionPassDebugStats::default(),
            teardown: None,
        }
    }

    /// Returns the root group key captured from the most recent `render()` call,
    /// or `None` before the first render.
    pub fn root_key(&self) -> Option<Key> {
        self.root_key
    }

    /// Sets the budget and the eviction order for groups retained when they leave the composition.
    /// See `RetentionMode`.
    pub fn set_retention_policy(&self, policy: RetentionPolicy) {
        self.composer_state.set_retention_policy(policy);
    }

    fn slots_host(&self) -> Rc<SlotsHost> {
        Rc::clone(&self.slots)
    }

    fn applier_host(&self) -> Rc<dyn ApplierHost> {
        Rc::<ConcreteApplierHost<A>>::clone(&self.applier)
    }

    fn reset_last_pass_stats(&mut self) {
        self.last_pass_stats = CompositionPassDebugStats::default();
    }

    #[expect(clippy::print_stderr, reason = "COMPOSE_DEBUG_SLOT_TABLE asks for it")]
    fn maybe_dump_slot_table(&self, label: &str) {
        if !DIAGNOSTICS.compose_debug_slot_table.is_set() {
            return;
        }
        eprintln!(
            "[COMPOSE_DEBUG_SLOT_TABLE] {label}\n{:#?}",
            self.debug_slot_snapshot()
        );
    }

    /// Returns whether a root render was requested and clears the request.
    pub fn take_root_render_request(&mut self) -> bool {
        std::mem::take(&mut self.root_render_requested)
    }

    /// Requests another render of the root content and schedules a frame for it.
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
        if !self.runtime.has_updates()
            && !self.runtime.has_invalid_scopes()
            && !self.runtime.has_frame_callbacks()
            && !self.runtime.has_pending_ui()
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

    /// Notes a finished pass's queue and side effects in the debug stats,
    /// then applies its commands.
    fn apply_pass(
        &mut self,
        host: &Rc<SlotsHost>,
        runtime_handle: &RuntimeHandle,
        composer: &Composer,
        side_effects: &Vec<Box<dyn FnOnce()>>,
    ) -> Result<(), NodeError> {
        composer.with_commands(|commands| self.record_pass_stats(commands, side_effects));
        self.apply_commands_and_updates_for_host(host, runtime_handle, composer)
    }

    /// Applies the commands `composer`'s pass queued, in its queue, which
    /// keeps its storage for the next pass, and then the runtime's updates.
    fn apply_commands_and_updates_for_host(
        &mut self,
        host: &Rc<SlotsHost>,
        runtime_handle: &RuntimeHandle,
        composer: &Composer,
    ) -> Result<(), NodeError> {
        let result = composer.with_commands(|commands| {
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
        });
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
                Rc::<ConcreteApplierHost<A>>::clone(&self.applier),
                runtime_handle.clone(),
                self.root,
            );
            let (root, side_effects, compact_applier) = composer.install(|composer| {
                let ((), outcome) = composer.try_with_slot_host_pass(
                    Rc::clone(&self.slots),
                    crate::slot::SlotPassMode::Compose,
                    |composer| composer.with_group(key, |_| content()),
                )?;
                let root = composer.root();
                let side_effects = composer.take_side_effects();
                Ok((root, side_effects, outcome.compacted))
            })?;
            self.apply_pass(
                &Rc::clone(&self.slots),
                &runtime_handle,
                &composer,
                &side_effects,
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

    /// Composes `content` as the root group identified by `key`, then recomposes the scopes that
    /// became invalid during the pass.
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

    /// Returns a handle to the runtime this composition uses.
    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }

    /// The runtime this composition runs on, to ask without a handle.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Borrows the applier mutably. Panics if it is already borrowed.
    pub fn applier_mut(&mut self) -> ApplierGuard<'_, A> {
        self.applier.borrow_typed()
    }

    /// Returns the root node a pass produced, or `None` if none has yet.
    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Returns one `(group index, call site key, scope id, subtree length)` tuple per group in the
    /// slot table, for debugging.
    pub fn debug_dump_slot_table_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.slots.borrow().debug_dump_groups()
    }

    /// Returns the entries of the slot table, for debugging.
    pub fn debug_dump_slot_entries(&self) -> Vec<crate::SlotDebugEntry> {
        self.slots.borrow().debug_dump_slot_entries()
    }

    /// Returns the heap memory in bytes that the slot table holds.
    pub fn slot_table_heap_bytes(&self) -> usize {
        self.slots.borrow().heap_bytes()
    }

    /// Returns size counters of the slot table, for debugging.
    pub fn debug_slot_table_stats(&self) -> SlotTableDebugStats {
        self.slots.debug_stats()
    }

    /// Returns a copy of the slot table's structure, for debugging.
    pub fn debug_slot_snapshot(&self) -> SlotDebugSnapshot {
        self.slots.debug_snapshot()
    }

    /// Returns the buffer sizes of the last composition pass, for debugging.
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

    /// The scopes grouped by the slot host each recomposes in, in the order
    /// each host first appears.
    /// The slots host `scope` recomposes in.
    fn host_for_scope(&self, scope: &RecomposeScope, root_host: &Rc<SlotsHost>) -> Rc<SlotsHost> {
        scope
            .slots_runtime_state()
            .and_then(|state| {
                scope
                    .slots_storage_key()
                    .and_then(|storage_key| state.host_for_storage_key(storage_key))
            })
            .or_else(|| {
                scope
                    .slots_storage_key()
                    .and_then(|storage_key| self.composer_state.host_for_storage_key(storage_key))
            })
            .unwrap_or_else(|| Rc::clone(root_host))
    }

    /// The slots host of each scope, with its scopes in order.
    fn group_scopes_by_host(
        &self,
        scopes: Vec<RecomposeScope>,
    ) -> SmallVec<[(Rc<SlotsHost>, Vec<RecomposeScope>); 1]> {
        let root_host = self.slots_host();
        // Scopes of one slots identity share a host: the usual frame keeps
        // its scope list as the one group.
        let first_identity = scopes.first().map(RecomposeScope::slots_host_identity);
        if let Some(first) = scopes.first()
            && scopes
                .iter()
                .all(|scope| Some(scope.slots_host_identity()) == first_identity)
        {
            let host = self.host_for_scope(first, &root_host);
            return smallvec::smallvec![(host, scopes)];
        }
        let mut scope_groups: SmallVec<[(Rc<SlotsHost>, Vec<RecomposeScope>); 1]> = SmallVec::new();
        let mut scope_group_index: HashMap<usize, usize> = HashMap::default();
        let mut previous_host: Option<(_, usize)> = None;
        for scope in scopes {
            let identity = scope.slots_host_identity();
            if let Some((previous_identity, index)) = previous_host
                && previous_identity == identity
            {
                scope_groups[index].1.push(scope);
                continue;
            }
            let host = self.host_for_scope(&scope, &root_host);
            let host_key = host.storage_key();
            let index = if let Some(index) = scope_group_index.get(&host_key).copied() {
                scope_groups[index].1.push(scope);
                index
            } else {
                scope_group_index.insert(host_key, scope_groups.len());
                scope_groups.push((host, vec![scope]));
                scope_groups.len() - 1
            };
            previous_host = Some((identity, index));
        }
        scope_groups
    }

    #[expect(clippy::print_stderr, reason = "a scope telemetry threshold asks")]
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
            self.dispose_forgotten_movables()?;
            let Some(scopes) = runtime_handle.take_invalidated_scopes() else {
                break;
            };
            if scopes.is_empty() {
                continue;
            }
            did_recompose |= recomposes_content(&scopes);
            let runtime_clone = runtime_handle.clone();
            let scope_groups = self.group_scopes_by_host(scopes);
            let mut host_group_index = 0usize;
            while host_group_index < scope_groups.len() {
                let (host, scopes) = &scope_groups[host_group_index];
                let scope_telemetry_threshold_ms =
                    DIAGNOSTICS.recompose_scope_telemetry_ms.threshold_ms();
                let shared_state = host
                    .runtime_state()
                    .or_else(|| scopes.first().and_then(RecomposeScope::slots_runtime_state))
                    .unwrap_or_else(|| Rc::clone(&self.composer_state));
                let side_effects = {
                    let _teardown = runtime::enter_state_teardown_scope();
                    let composer = Composer::for_pass(
                        self.parked_composer.take(),
                        shared_state,
                        Rc::clone(host),
                        self.applier_host(),
                        runtime_clone.clone(),
                        self.root,
                    );
                    composer.parent_stack().clear();
                    let (root, side_effects, requested_root_render, compact_applier) =
                        composer.install(|composer| {
                            let ((), outcome) = composer.try_with_slot_host_pass(
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
                            let side_effects = composer.take_side_effects();
                            let requested_root_render = composer.take_root_render_request();
                            Ok((
                                root,
                                side_effects,
                                requested_root_render,
                                outcome.compacted,
                            ))
                        })?;
                    self.apply_pass(host, &runtime_handle, &composer, &side_effects)?;
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
                    self.park_composer(host, composer);
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
                            runtime_handle.requeue_invalid_scope(scope);
                        }
                    }
                    break;
                }
                host_group_index += 1;
            }
            recycle_scope_groups(&runtime_handle, scope_groups);
            if self.root_render_requested {
                break;
            }
        }
        self.finalize_runtime_state();
        Ok(did_recompose)
    }

    /// Recomposes the scopes that state changes invalidated, until none remain or a root render is
    /// requested. Returns `true` if a pass recomposed content.
    pub fn process_invalid_scopes(&mut self) -> Result<bool, NodeError> {
        self.process_invalid_scopes_until_root_request()
    }

    /// Keeps the composer of a pass over the root slots for the next pass.
    fn park_composer(&mut self, host: &Rc<SlotsHost>, composer: Composer) {
        if Rc::ptr_eq(host, &self.slots) {
            self.parked_composer = composer.park();
        }
    }

    fn dispose_forgotten_movables(&mut self) -> Result<(), NodeError> {
        let ids = self.runtime.take_forgotten_movables();
        if ids.is_empty() {
            return Ok(());
        }
        let runtime_handle = self.runtime_handle();
        let _teardown = runtime::enter_state_teardown_scope();
        let host = self.slots_host();
        let composer = Composer::new_with_shared_state(
            Rc::clone(&self.composer_state),
            Rc::clone(&host),
            self.applier_host(),
            runtime_handle.clone(),
            self.root,
        );
        composer.install(|composer| composer.forget_movables(&ids))?;
        self.apply_commands_and_updates_for_host(&host, &runtime_handle, &composer)
    }

    /// Applies the node updates that the runtime has queued to the applier, without recomposing.
    pub fn flush_pending_node_updates(&mut self) -> Result<(), NodeError> {
        let updates = self.runtime.take_updates();
        let mut applier = self.applier.borrow_dyn();
        for update in updates {
            update.apply(&mut *applier)?;
        }
        Ok(())
    }
}

/// Whether running these scopes can change what the composition shows: a pass
/// that only recomputes derived states changes nothing unless a derived value
/// moved, and then its readers run in a pass of their own.
fn recomposes_content(scopes: &[RecomposeScope]) -> bool {
    scopes.iter().any(|scope| !scope.is_derivation())
}

/// Gives a pass's scope lists back to the runtime, for the next pass to
/// reuse their storage.
fn recycle_scope_groups(
    runtime: &RuntimeHandle,
    scope_groups: SmallVec<[(Rc<SlotsHost>, Vec<RecomposeScope>); 1]>,
) {
    for (_, scopes) in scope_groups {
        runtime.recycle_scopes(scopes);
    }
}

impl<A: Applier + 'static> Drop for Composition<A> {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        self.runtime.restore_thread_state();
        self.teardown = Some(runtime::enter_state_teardown_scope());
    }
}

/// The largest sizes that the command queues and the side effect list reached during one
/// composition pass, for debugging memory use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompositionPassDebugStats {
    /// Most node commands queued at once.
    pub commands_len: usize,
    /// Largest capacity reserved for node commands.
    pub commands_cap: usize,
    /// Most bytes of command arguments queued at once.
    pub command_payload_len_bytes: usize,
    /// Largest capacity reserved for command arguments, in bytes.
    pub command_payload_cap_bytes: usize,
    /// Most child list updates queued at once.
    pub sync_children_len: usize,
    /// Largest capacity reserved for child list updates.
    pub sync_children_cap: usize,
    /// Most child ids stored for child list updates.
    pub sync_child_ids_len: usize,
    /// Largest capacity reserved for those child ids.
    pub sync_child_ids_cap: usize,
    /// Most side effects queued at once.
    pub side_effects_len: usize,
    /// Largest capacity reserved for the side effects.
    pub side_effects_cap: usize,
}

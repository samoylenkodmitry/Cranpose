use std::{cell::Cell, hash::Hash, rc::Rc};

use super::{GroupScopeEntry, PendingMovable, SlotHostPassGuard};
use crate::{
    BodyRun, Composer, Key, RecomposeOptions, RecomposeScope, RetentionMode, SlotsHost,
    explicit_group_key_seed, slot::GroupStart,
};

/// A group a composable call opened; [`ComposableGroup::close`] ends it
/// after the call's body.
#[doc(hidden)]
pub struct ComposableGroup<'a> {
    composer: &'a Composer,
    host: Rc<SlotsHost>,
    scope: RecomposeScope,
    pass: Option<SlotHostPassGuard>,
    runs_body: bool,
    completed: bool,
    skipped: Cell<bool>,
}

impl ComposableGroup<'_> {
    /// The recompose scope of the group.
    pub fn scope(&self) -> &RecomposeScope {
        &self.scope
    }

    /// Registers `body` as the function that recomposes the group's scope,
    /// as [`Composer::set_recompose_fn`] does for the current scope: a plain
    /// function needs no box and no call shim.
    #[inline]
    pub fn set_recompose_fn(&self, body: fn(&Composer)) {
        self.scope.set_recompose_fn(body);
    }

    /// Skips the group's body: what it composed last stays. The call
    /// composes nothing more in the group before closing it.
    pub fn skip(&self) {
        self.skipped.set(true);
    }

    /// Ends the group after its body composed.
    #[inline]
    pub fn close(mut self) {
        self.completed = true;
    }
}

impl Drop for ComposableGroup<'_> {
    fn drop(&mut self) {
        if self.completed {
            self.scope.mark_composed_once();
        }
        let ran = self.completed & self.runs_body & !self.skipped.get();
        self.scope.end_reads(BodyRun::from(ran));
        if self.skipped.get() {
            self.composer
                .close_skipped_group_in_active_pass(&self.host, &self.scope);
        } else {
            self.composer
                .close_group_in_active_pass(&self.host, &self.scope);
        }
        if let Some(mut pass) = self.pass.take() {
            if self.completed
                && let Err(err) = self.composer.finish_slot_host_pass(&pass.host)
            {
                log::error!("slot host pass finalization failed: {err}");
            }
            pass.close();
        }
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl Composer {
    fn scope_for_started_group(
        &self,
        host: &SlotsHost,
        group: crate::slot::ActiveGroupId,
        scope: Option<RecomposeScope>,
    ) -> RecomposeScope {
        if let Some(scope) = scope {
            return scope;
        }
        let scope = RecomposeScope::new(self.runtime_handle());
        host.with_write_session(|slots| slots.set_group_scope(group, scope.clone()));
        scope
    }

    #[inline(never)]
    fn open_group_in_active_pass(
        &self,
        host: &Rc<SlotsHost>,
        key: crate::slot::GroupKeySeed,
    ) -> (RecomposeScope, bool) {
        let options = self.core.pending_scope_options.take().unwrap_or_default();
        let parent_node = self.current_parent_hint();
        let (
            GroupStart {
                group,
                anchor,
                scope,
                kind,
            },
            placeholder_for,
        ) = self.start_group(host, key, parent_node);
        let scope_ref = self.scope_for_started_group(host, group, scope);
        if let Some(movable_key) = placeholder_for {
            self.core.shared_state.record_pending_movable(
                host,
                PendingMovable {
                    key: movable_key,
                    placeholder: anchor,
                    parent_scope: self
                        .core
                        .scope_stack
                        .borrow()
                        .last()
                        .map(RecomposeScope::downgrade),
                },
            );
        }
        self.enter_group_scope(
            &scope_ref,
            GroupScopeEntry {
                options,
                start_kind: kind,
                host,
                group,
                parent_hint: parent_node,
            },
        );
        (scope_ref, placeholder_for.is_none())
    }

    #[inline(never)]
    fn close_group_in_active_pass(&self, host: &Rc<SlotsHost>, scope: &RecomposeScope) {
        let result = host.with_write_session(|slots| {
            let result = slots.finish_group_body_changes();
            slots.end_group();
            result
        });
        self.close_finished_group(host, scope, result);
        self.settle_closed_group(scope);
    }

    /// Closes a group whose body was skipped, in one slot session: its root
    /// nodes stay attached where they were.
    #[inline(never)]
    fn close_skipped_group_in_active_pass(&self, host: &Rc<SlotsHost>, scope: &RecomposeScope) {
        // Attaching a node reaches the parent stack and the applier, never
        // the slot table the session holds.
        host.with_write_session(|slots| slots.skip_and_end_group(|id| self.attach_to_parent(id)));
        self.pop_closed_scope(scope);
        self.settle_closed_group(scope);
    }

    fn settle_closed_group(&self, scope: &RecomposeScope) {
        scope.mark_recomposed();
        if let Err(err) = self.flush_pending_commands_if_large() {
            log::error!("mid-composition command flush failed: {err}");
        }
    }

    #[inline(never)]
    fn open_group(&self, key: crate::slot::GroupKeySeed) -> ComposableGroup<'_> {
        let host = self.active_slots_host();
        let (host, pass) = if host.has_active_pass() {
            (host, None)
        } else {
            let pass = self.begin_slot_host_pass(&host, crate::slot::SlotPassMode::Compose);
            (self.active_slots_host(), Some(pass))
        };
        let (scope, runs_body) = self.open_group_in_active_pass(&host, key);
        scope.begin_reads();
        ComposableGroup {
            composer: self,
            host,
            scope,
            pass,
            runs_body,
            completed: false,
            skipped: Cell::new(false),
        }
    }

    #[doc(hidden)]
    pub fn __open_composable_group(&self, key: Key) -> ComposableGroup<'_> {
        self.open_group(crate::slot::GroupKeySeed::unkeyed(key))
    }

    pub(crate) fn with_group_seed<R>(
        &self,
        key: crate::slot::GroupKeySeed,
        f: impl FnOnce(&Self) -> R,
    ) -> R {
        let group = self.open_group(key);
        debug_assert!(group.runs_body, "only movable content waits for a body");
        let result = f(self);
        group.close();
        result
    }

    pub(crate) fn with_movable_group(&self, id: Key, f: impl FnOnce(&Self)) {
        let group = self.open_group(crate::slot::GroupKeySeed::movable(id));
        if group.runs_body {
            f(self);
        }
        group.close();
    }

    /// Runs `f` in a group identified by `key`. Slots and nodes that `f` creates belong to the
    /// group.
    pub fn with_group<R>(&self, key: Key, f: impl FnOnce(&Self) -> R) -> R {
        self.with_group_seed(crate::slot::GroupKeySeed::unkeyed(key), f)
    }

    /// Runs `f` in a group identified by `key` that is retained when it leaves the composition,
    /// with the recompose `options` applied to its scope. A retained group returns with its state
    /// and nodes when `key` is composed again.
    pub fn cranpose_with_reuse<R>(
        &self,
        key: Key,
        mut options: RecomposeOptions,
        f: impl FnOnce(&Self) -> R,
    ) -> R {
        options.retention = RetentionMode::RetainWhenInactive;
        self.core.pending_scope_options.set(Some(options));
        self.with_group(key, f)
    }

    /// Runs `f` in a group identified by the hash of `key`, so state and nodes follow the key when
    /// siblings move. Compose's `key`.
    #[track_caller]
    pub fn with_key<K: Hash, R>(&self, key: &K, f: impl FnOnce(&Self) -> R) -> R {
        let seed = explicit_group_key_seed(key, std::panic::Location::caller());
        self.with_group_seed(seed, f)
    }
}

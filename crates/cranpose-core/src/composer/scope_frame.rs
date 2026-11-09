//! The frames of a composer's scope stack: a scope, or a group that makes
//! its scope only when its body first needs one.

use super::{GroupScopeEntry, reparent_restored_scopes};
use crate::{
    AnchorId, Composer, LocalStackSnapshot, NodeId, RecomposeOptions, RecomposeScope,
    RetentionMode, ScopeRead, StateId,
    slot::{ActiveGroupId, GroupStartKind},
};

// A frame of the composer's scope stack: a group's scope, the scope a
// restarted body or a callback runs in, or a group whose body makes its
// scope when it first needs one.
pub(crate) enum ScopeFrame {
    Made(RecomposeScope),
    Deferred(DeferredScope),
}

// What a group without a scope keeps to make one: most bodies read no
// state, take no callback and ask for no scope, and their groups keep none.
pub(crate) struct DeferredScope {
    pub(crate) group: ActiveGroupId,
    anchor: AnchorId,
    parent_hint: Option<NodeId>,
    locals: LocalStackSnapshot,
    pub(crate) recompose: Option<fn(&Composer)>,
    // Whether the body runs whatever its parameters: a changed static
    // composition local, or reused content around it.
    pub(crate) forced: bool,
    // Whether the group composed before this pass.
    pub(crate) composed_once: bool,
    // The scope the body made.
    pub(crate) made: Option<RecomposeScope>,
}

impl ScopeFrame {
    pub(crate) fn scope(&self) -> Option<&RecomposeScope> {
        match self {
            Self::Made(scope) => Some(scope),
            Self::Deferred(deferred) => deferred.made.as_ref(),
        }
    }

    // Whether a state read in the frame recomposes it: a group of a
    // composable, which recomposes through its function, or a scope with a
    // hand-written body.
    fn has_recompose_callback(&self) -> bool {
        match self {
            Self::Made(scope) => scope.has_recompose_callback(),
            Self::Deferred(deferred) => match &deferred.made {
                Some(scope) => scope.has_recompose_callback(),
                None => deferred.recompose.is_some(),
            },
        }
    }
}

// Whether a group opened with `options` and `kind` waits for its body to
// make its scope.
pub(super) fn may_defer(options: RecomposeOptions, kind: GroupStartKind) -> bool {
    !options.force_reuse
        && !options.force_recompose
        && matches!(options.retention, RetentionMode::DisposeWhenInactive)
        && !matches!(kind, GroupStartKind::Restored)
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl Composer {
    /// Returns the recompose scope being composed, or `None` outside any scope.
    pub fn current_recompose_scope(&self) -> Option<RecomposeScope> {
        let mut stack = self.core.scope_stack.borrow_mut();
        let top = stack.len().checked_sub(1)?;
        Some(self.frame_scope(&mut stack, top))
    }

    // The scope being composed when it has one: a group whose body made
    // none yet gives `None`, and makes none.
    pub(crate) fn peek_recompose_scope(&self) -> Option<RecomposeScope> {
        self.core
            .scope_stack
            .borrow()
            .last()
            .and_then(ScopeFrame::scope)
            .cloned()
    }

    pub(crate) fn note_state_read(
        &self,
        state_id: Option<StateId>,
    ) -> Option<(RecomposeScope, ScopeRead)> {
        let scope = {
            let mut stack = self.core.scope_stack.borrow_mut();
            let index = stack
                .iter()
                .rposition(ScopeFrame::has_recompose_callback)
                .or_else(|| stack.len().checked_sub(1))?;
            self.frame_scope(&mut stack, index)
        };
        let read = state_id.map_or(ScopeRead::NotReading, |id| scope.note_read(id));
        (!matches!(read, ScopeRead::Watched)).then_some((scope, read))
    }

    // Whether a group opened now waits for its body to make its scope: not
    // a group with no group around it, one in pausable content, or the
    // first group of a subcomposition slot, whose scope the slot keeps.
    pub(super) fn defers_scopes_here(&self) -> bool {
        !self.core.scope_stack.borrow().is_empty()
            && !self.core.shared_state.pausing.is_holding()
            && self
                .subcompose_stack()
                .last()
                .is_none_or(|frame| !frame.scopes.is_empty())
    }

    // Opens the scope frame of a group that makes its scope when its body
    // first needs one.
    pub(super) fn push_deferred_scope(
        &self,
        (group, anchor): (ActiveGroupId, AnchorId),
        kind: GroupStartKind,
        parent_hint: Option<NodeId>,
    ) {
        let forced = self.core.locals_changed.get() || self.core.reused_depth.get() > 0;
        let frame = DeferredScope {
            group,
            anchor,
            parent_hint,
            locals: self.core.local_stack.borrow().clone(),
            recompose: None,
            forced,
            composed_once: !matches!(kind, GroupStartKind::Inserted),
            made: None,
        };
        self.scope_stack().push(ScopeFrame::Deferred(frame));
    }

    // The scope of the frame at `index`, made now for a group that has
    // none yet.
    pub(super) fn frame_scope(&self, stack: &mut [ScopeFrame], index: usize) -> RecomposeScope {
        let owner = stack[..index]
            .iter()
            .rev()
            .find_map(ScopeFrame::scope)
            .cloned();
        let deferred = match &mut stack[index] {
            ScopeFrame::Made(scope) => return scope.clone(),
            ScopeFrame::Deferred(deferred) => deferred,
        };
        if let Some(scope) = &deferred.made {
            return scope.clone();
        }
        let scope = RecomposeScope::new(self.runtime_handle());
        match &owner {
            Some(owner) => scope.set_parent_scope(Some(owner)),
            None => scope
                .set_lifetime_owner_scope(self.core.subcomposition_owner_scope.borrow().as_ref()),
        }
        // The scope learns its slot host when its group closes: a body may
        // read a state while the host's table is open for a write.
        scope.set_group_anchor(deferred.anchor);
        scope.snapshot_locals(&deferred.locals);
        scope.set_parent_hint(deferred.parent_hint);
        if let Some(recompose) = deferred.recompose {
            scope.set_recompose_fn(recompose);
        }
        if deferred.composed_once {
            scope.mark_composed_once();
        }
        scope.begin_reads();
        if let Some(frame) = self.subcompose_stack().last_mut() {
            frame.scopes.push(scope.clone());
        }
        deferred.made = Some(scope.clone());
        scope
    }

    // Enters `scope_ref`, the scope of a group opened with it; returns
    // whether the group forces the groups below it to run their bodies, as
    // reused content does.
    pub(super) fn enter_group_scope(
        &self,
        scope_ref: &RecomposeScope,
        entry: GroupScopeEntry<'_>,
    ) -> bool {
        let GroupScopeEntry {
            options,
            start_kind,
            host,
            group,
            parent_hint,
        } = entry;
        // A group entered while its scope is inactive is reused content, as
        // when a lazy list gives a recycled row to another item: like Compose,
        // it composes again rather than skipping, which reactivates the
        // scopes beneath it.
        let reused = !scope_ref.is_active() && scope_ref.has_composed_once();
        scope_ref.reactivate();
        {
            let mut stack = self.scope_stack();
            // Content a group keeps when it leaves is found again by its
            // parent's scope.
            if matches!(options.retention, RetentionMode::RetainWhenInactive)
                && let Some(top) = stack.len().checked_sub(1)
            {
                self.frame_scope(&mut stack, top);
            }
            match stack.iter().rev().find_map(ScopeFrame::scope) {
                Some(parent_scope) => scope_ref.set_parent_scope(Some(parent_scope)),
                None => scope_ref.set_lifetime_owner_scope(
                    self.core.subcomposition_owner_scope.borrow().as_ref(),
                ),
            }
            stack.push(ScopeFrame::Made(scope_ref.clone()));
        }
        scope_ref.set_retention_mode(options.retention);

        // Content under a changed static composition local runs every body,
        // as restored content does.
        let restored = matches!(start_kind, GroupStartKind::Restored);
        let recompose = options.force_recompose | self.core.locals_changed.get() | restored;
        // Reused content runs every body below it, whose scopes it left
        // inactive; restored content reactivates the scopes below it.
        let forces_content = reused & !restored & !options.force_reuse;
        if recompose | forces_content {
            scope_ref.force_recompose();
        } else if options.force_reuse {
            scope_ref.force_reuse();
        }
        if forces_content {
            let depth = &self.core.reused_depth;
            depth.set(depth.get() + 1);
        }

        scope_ref.set_slots_host(host);

        {
            let mut stack = self.subcompose_stack();
            if let Some(frame) = stack.last_mut() {
                frame.scopes.push(scope_ref.clone());
            }
        }

        scope_ref.snapshot_locals(&self.core.local_stack.borrow());
        if restored {
            reparent_restored_scopes(host, group, scope_ref.parent_hint(), parent_hint);
        }
        scope_ref.set_parent_hint(parent_hint);
        forces_content
    }

    // The deferred frame of the group opened last, whose body has not run.
    fn with_last_deferred<R>(&self, read: fn(&mut DeferredScope) -> R) -> Option<R> {
        match self.scope_stack().last_mut() {
            Some(ScopeFrame::Deferred(deferred)) => Some(read(deferred)),
            _ => None,
        }
    }

    // Whether the group opened last, which waits for its body to make its
    // scope, runs its body whatever its parameters.
    pub(super) fn deferred_scope_forced(&self) -> bool {
        self.with_last_deferred(|deferred| deferred.forced)
            .unwrap_or(false)
    }

    // Whether the group opened last, which waits for its body to make its
    // scope, composed before.
    pub(super) fn deferred_scope_composed_once(&self) -> bool {
        self.with_last_deferred(|deferred| deferred.composed_once)
            .unwrap_or(false)
    }

    // Gives the group opened last, which waits for its body to make its
    // scope, the function that recomposes the scope it makes.
    pub(super) fn set_deferred_recompose_fn(&self, body: fn(&Self)) {
        if let Some(ScopeFrame::Deferred(deferred)) = self.scope_stack().last_mut() {
            match &deferred.made {
                Some(scope) => scope.set_recompose_fn(body),
                None => deferred.recompose = Some(body),
            }
        }
    }

    // The scope the body of the group that closes now made, with the group.
    pub(super) fn scope_made_by_body(&self) -> Option<(ActiveGroupId, RecomposeScope)> {
        match self.scope_stack().last() {
            Some(ScopeFrame::Deferred(deferred)) => deferred
                .made
                .as_ref()
                .map(|scope| (deferred.group, scope.clone())),
            _ => None,
        }
    }

    // Leaves a group that forced the groups below it to run their bodies.
    pub(super) fn leave_forced_content(&self) {
        let depth = &self.core.reused_depth;
        depth.set(depth.get().saturating_sub(1));
    }

    // Pops the frame of a group that closed.
    pub(crate) fn pop_closed_scope(&self) {
        if self.scope_stack().pop().is_none() {
            log::error!("scope stack underflow while closing a group");
        }
    }
}

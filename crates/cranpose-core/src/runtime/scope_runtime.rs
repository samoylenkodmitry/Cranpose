//! The link a recompose scope keeps to its runtime.

use std::rc::Weak;

use super::{RuntimeInner, StateId};
use crate::{RecomposeScopeInner, ScopeId};

/// What a recompose scope keeps of its runtime: one word, where a
/// [`RuntimeHandle`](super::RuntimeHandle) takes four, on every scope of a composition. Each call
/// does nothing once the runtime is gone, as the handle's would.
pub(crate) struct ScopeRuntime(pub(super) Weak<RuntimeInner>);

impl ScopeRuntime {
    // Counts a frame in which bodies of pausable content that waited run as
    // one invalid scope, so the composition runs a pass in it, until
    // `mark_scope_recomposed` gives it back.
    pub(crate) fn hold_due_bodies(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner
                .invalid_scope_count
                .set(inner.invalid_scope_count.get() + 1);
        }
    }

    pub(crate) fn mark_scope_recomposed(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.mark_scope_recomposed();
        }
    }

    pub(crate) fn register_invalid_scope(&self, scope: Weak<RecomposeScopeInner>) {
        if let Some(inner) = self.0.upgrade() {
            inner.register_invalid_scope(scope);
        }
    }

    pub(crate) fn unregister_state_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(inner) = self.0.upgrade() {
            inner.state_arena.unregister_scope(id, scope_id);
        }
    }

    pub(crate) fn forget_state_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(inner) = self.0.upgrade() {
            inner.state_arena.forget_scope(id, scope_id);
        }
    }

    pub(crate) fn increment_live_recompose_scope_count(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.increment_live_recompose_scope_count();
        }
    }

    pub(crate) fn decrement_live_recompose_scope_count(&self) {
        if let Some(inner) = self.0.upgrade() {
            inner.decrement_live_recompose_scope_count();
        }
    }
}

use std::{
    cell::{Cell, RefCell},
    rc::Weak,
};

use smallvec::SmallVec;

use super::Subscribers;
use crate::{
    RecomposeScope, RecomposeScopeInner, RuntimeHandle, ScopeId, StateId,
    collections::map::HashMap, debug_trace::debug_record_scope_invalidation,
    with_current_composer_opt,
};

// The scopes that read a state and the runtime they recompose in, whatever
// the state's value type, so that registering a reader and invalidating the
// readers are compiled once for every state type.
pub(super) struct StateWatchers {
    pub(super) scopes: RefCell<HashMap<ScopeId, Weak<RecomposeScopeInner>>>,
    pub(super) runtime: RuntimeHandle,
    pub(super) state_id: Cell<Option<StateId>>,
}

fn shrink_watchers_if_sparse(watchers: &mut HashMap<ScopeId, Weak<RecomposeScopeInner>>) {
    let len = watchers.len();
    let capacity = watchers.capacity();
    if capacity > len.saturating_mul(4).max(32) {
        watchers.shrink_to_fit();
    }
}

impl StateWatchers {
    /// Adds `scope` to the state's watchers, saying whether it was new and
    /// whether the state thereby gained its first subscriber.
    ///
    /// Dead watchers still count as subscribers until pruned. They are
    /// pruned whenever no live one is left, so a reader arriving after every
    /// earlier one died still makes the state newly subscribed, and
    /// otherwise only when the map is full, before the insert would grow
    /// it: a state read by thousands of scopes, such as the density, must
    /// not scan them all for every new reader.
    pub(super) fn register_scope(
        &self,
        subscribers: &Subscribers,
        scope: &RecomposeScope,
    ) -> (bool, bool) {
        let mut watchers = self.scopes.borrow_mut();
        let id = scope.id();
        match watchers.get(&id) {
            Some(existing) if existing.strong_count() > 0 => return (false, false),
            Some(_) => {
                watchers.remove(&id);
                subscribers.remove_scope_observers(1);
            }
            None => {}
        }
        let any_live = watchers.values().any(|watcher| watcher.strong_count() > 0);
        if !any_live || watchers.len() >= watchers.capacity() {
            let before = watchers.len();
            watchers.retain(|_, watcher| watcher.strong_count() > 0);
            subscribers.remove_scope_observers(before - watchers.len());
        }
        watchers.insert(id, scope.downgrade());
        drop(watchers);
        (true, subscribers.add_scope_observer())
    }

    pub(super) fn has_subscribers(&self, subscribers: &Subscribers) -> bool {
        let mut watchers = self.scopes.borrow_mut();
        let before = watchers.len();
        watchers.retain(|_, existing| existing.upgrade().is_some());
        subscribers.remove_scope_observers(before - watchers.len());
        subscribers.has_subscribers()
    }

    pub(super) fn forget_scope(&self, subscribers: &Subscribers, scope_id: ScopeId) {
        let mut watchers = self.scopes.borrow_mut();
        let removed = watchers.remove(&scope_id).is_some();
        if removed {
            shrink_watchers_if_sparse(&mut watchers);
        }
        drop(watchers);
        subscribers.remove_scope_observers(usize::from(removed));
    }

    pub(super) fn unregister_scope(&self, subscribers: &Subscribers, scope_id: ScopeId) {
        let mut watchers = self.scopes.borrow_mut();
        let removed = if watchers
            .get(&scope_id)
            .is_some_and(|weak| weak.upgrade().is_none())
        {
            watchers.remove(&scope_id);
            shrink_watchers_if_sparse(&mut watchers);
            true
        } else {
            false
        };
        drop(watchers);
        subscribers.remove_scope_observers(usize::from(removed));
    }

    // Invalidates the live scopes that read the state, whose value type is
    // named `type_name`, and drops the dead ones.
    pub(super) fn invalidate(&self, subscribers: &Subscribers, type_name: &'static str) {
        // A state has few watchers: their copy stays inline.
        let (watchers, removed_count): (SmallVec<[RecomposeScope; 4]>, usize) = {
            let mut watchers = self.scopes.borrow_mut();
            let before = watchers.len();
            let mut live = SmallVec::new();
            watchers.retain(|_, scope| {
                if let Some(inner) = scope.upgrade() {
                    live.push(RecomposeScope { inner });
                    true
                } else {
                    false
                }
            });
            let removed_count = before - watchers.len();
            shrink_watchers_if_sparse(&mut watchers);
            (live, removed_count)
        };
        subscribers.remove_scope_observers(removed_count);

        for watcher in watchers {
            debug_record_scope_invalidation(watcher.id(), self.state_id.get(), type_name);
            if let Some(state_id) = self.state_id.get() {
                watcher.invalidate_from_state(state_id);
            } else {
                watcher.invalidate();
            }
        }
    }
}

// Out of line, one function for every state type: each read of a state
// registers its reader here.
#[inline(never)]
pub(super) fn register_current_state_scope(watchers: &StateWatchers, subscribers: &Subscribers) {
    let state_id = watchers.state_id.get();
    let Some(Some((scope, read))) =
        with_current_composer_opt(|composer| composer.note_state_read(state_id))
    else {
        return;
    };
    let (registered, became_subscribed) = watchers.register_scope(subscribers, &scope);
    if let (crate::ScopeRead::NotReading, true, Some(state_id)) = (read, registered, state_id) {
        scope.record_state_subscription(state_id);
    }
    if became_subscribed {
        subscribers.notify_subscribers();
    }
}

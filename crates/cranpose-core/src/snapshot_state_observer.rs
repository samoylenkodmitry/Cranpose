// Complex types are inherent to the observer pattern with nested callbacks and state tracking
#![allow(clippy::type_complexity)]

use crate::collections::map::{HashMap, HashSet};
use crate::snapshot_v2::{register_apply_observer, ReadObserver, StateObjectId};
use crate::state::StateObject;
use crate::{RecomposeScope, RecomposeScopeInner, ScopeId};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

/// Executes a callback once changes are delivered.
type Executor = dyn Fn(Box<dyn FnOnce() + 'static>) + 'static;

/// Observer that records state object reads performed inside a given scope and
/// notifies the caller when any of the observed objects change.
///
/// This is a pragmatic Rust translation of Jetpack Compose's
/// `SnapshotStateObserver`. The implementation focuses on the core behaviour
/// needed by the Cranpose runtime:
/// - Tracking state object reads per logical scope.
/// - Reacting to snapshot apply notifications.
/// - Scheduling invalidation callbacks via the supplied executor.
///
/// Advanced features from the Kotlin version (derived state tracking, change
/// coalescing, queue minimisation) are deferred
#[derive(Clone)]
pub struct SnapshotStateObserver {
    inner: Rc<SnapshotStateObserverInner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnapshotStateObserverDebugStats {
    pub scopes_len: usize,
    pub scopes_cap: usize,
    pub fast_scopes_len: usize,
    pub fast_scopes_cap: usize,
    pub stateless_scope_count: usize,
    pub observed_state_count: usize,
    pub observed_state_capacity: usize,
}

impl SnapshotStateObserver {
    /// Create a new observer that schedules callbacks using `on_changed_executor`.
    pub fn new(on_changed_executor: impl Fn(Box<dyn FnOnce() + 'static>) + 'static) -> Self {
        let inner = Rc::new(SnapshotStateObserverInner::new(on_changed_executor));
        inner.set_self(Rc::downgrade(&inner));
        Self { inner }
    }

    /// Observe state object reads performed while executing `block`.
    ///
    /// Subsequent calls to `observe_reads` replace any previously recorded
    /// observations for the provided `scope`. When one of the observed objects
    /// mutates, `on_value_changed_for_scope` will be invoked on the executor.
    pub fn observe_reads<T, R>(
        &self,
        scope: T,
        on_value_changed_for_scope: impl Fn(&T) + 'static,
        block: impl FnOnce() -> R,
    ) -> R
    where
        T: Any + Clone + PartialEq + 'static,
    {
        self.inner
            .observe_reads(scope, on_value_changed_for_scope, block)
    }

    /// Notify the observer that a new composition frame is starting.
    pub fn begin_frame(&self) {
        self.inner.begin_frame();
    }

    /// Drop bookkeeping for scopes that were released during the current frame.
    pub fn prune_dead_scopes(&self) {
        self.inner.prune_dead_scopes();
    }

    /// Temporarily pause read observation while executing `block`.
    pub fn with_no_observations<R>(&self, block: impl FnOnce() -> R) -> R {
        self.inner.with_no_observations(block)
    }

    /// Remove any recorded reads for `scope`.
    pub fn clear<T>(&self, scope: &T)
    where
        T: Any + PartialEq + 'static,
    {
        self.inner.clear(scope);
    }

    /// Remove recorded reads for scopes that satisfy `predicate`.
    pub fn clear_if(&self, predicate: impl Fn(&dyn Any) -> bool) {
        self.inner.clear_if(predicate);
    }

    /// Remove all recorded observations.
    pub fn clear_all(&self) {
        self.inner.clear_all();
    }

    /// Begin listening for snapshot apply notifications.
    pub fn start(&self) {
        let weak = Rc::downgrade(&self.inner);
        self.inner.start(weak);
    }

    /// Stop listening for snapshot apply notifications.
    pub fn stop(&self) {
        self.inner.stop();
    }

    pub fn debug_stats(&self) -> SnapshotStateObserverDebugStats {
        self.inner.debug_stats()
    }

    /// Test-only helper to simulate snapshot changes.
    #[cfg(test)]
    pub fn notify_changes(&self, modified: &[Arc<dyn StateObject>]) {
        self.inner.handle_apply(modified);
    }
}

struct SnapshotStateObserverInner {
    executor: Rc<Executor>,
    owned_scopes: RefCell<Vec<Rc<RefCell<ScopeEntry>>>>,
    fast_scopes: RefCell<HashMap<ScopeId, Rc<RefCell<ScopeEntry>>>>,
    indexed_scopes: RefCell<HashMap<usize, Rc<RefCell<ScopeEntry>>>>,
    observed_to_scopes: RefCell<HashMap<StateObjectId, Vec<usize>>>,
    pause_count: Rc<Cell<usize>>,
    active_read_targets: Rc<RefCell<Vec<Rc<RefCell<ObservedIds>>>>>,
    read_dispatcher: ReadObserver,
    apply_handle: RefCell<Option<crate::snapshot_v2::ObserverHandle>>,
    weak_self: RefCell<Weak<SnapshotStateObserverInner>>,
    frame_version: Cell<u64>,
    next_entry_id: Cell<usize>,
}

impl SnapshotStateObserverInner {
    const MIN_RETAINED_SCOPE_CAPACITY: usize = 256;

    fn new(on_changed_executor: impl Fn(Box<dyn FnOnce() + 'static>) + 'static) -> Self {
        let pause_count = Rc::new(Cell::new(0));
        let active_read_targets = Rc::new(RefCell::new(Vec::<Rc<RefCell<ObservedIds>>>::new()));
        let dispatcher_pause_count = Rc::clone(&pause_count);
        let dispatcher_targets = Rc::clone(&active_read_targets);
        let read_dispatcher: ReadObserver = Arc::new(move |state| {
            if dispatcher_pause_count.get() > 0 {
                return;
            }
            let observed = dispatcher_targets.borrow().last().cloned();
            if let Some(observed) = observed {
                observed.borrow_mut().insert(state.object_id().as_usize());
            }
        });

        Self {
            executor: Rc::new(on_changed_executor),
            owned_scopes: RefCell::new(Vec::new()),
            fast_scopes: RefCell::new(HashMap::default()),
            indexed_scopes: RefCell::new(HashMap::default()),
            observed_to_scopes: RefCell::new(HashMap::default()),
            pause_count,
            active_read_targets,
            read_dispatcher,
            apply_handle: RefCell::new(None),
            weak_self: RefCell::new(Weak::new()),
            frame_version: Cell::new(0),
            next_entry_id: Cell::new(0),
        }
    }

    fn set_self(&self, weak: Weak<SnapshotStateObserverInner>) {
        self.weak_self.replace(weak);
    }

    fn begin_frame(&self) {
        let next = self.frame_version.get().wrapping_add(1);
        self.frame_version.set(next);
        self.prune_dead_scopes();
    }

    fn observe_reads<T, R>(
        &self,
        scope: T,
        on_value_changed_for_scope: impl Fn(&T) + 'static,
        block: impl FnOnce() -> R,
    ) -> R
    where
        T: Any + Clone + PartialEq + 'static,
    {
        let frame_version = self.frame_version.get();
        let has_frame_version = frame_version != 0;

        let on_changed: Rc<dyn Fn(&dyn Any)> = {
            let callback = Rc::new(on_value_changed_for_scope);
            Rc::new(move |scope_any: &dyn Any| {
                if let Some(typed) = scope_any.downcast_ref::<T>() {
                    callback(typed);
                }
            })
        };

        let existing_entry = self.find_scope_entry(&scope);
        if let Some(entry) = existing_entry.as_ref() {
            let already_observed = {
                let mut entry_mut = entry.borrow_mut();
                entry_mut.update(scope.clone(), on_changed.clone());
                has_frame_version && entry_mut.last_seen_version == frame_version
            };
            if already_observed {
                return block();
            }
        }

        let observed = Rc::new(RefCell::new(ObservedIds::new()));
        self.active_read_targets
            .borrow_mut()
            .push(Rc::clone(&observed));
        struct ActiveObservationGuard {
            stack: Rc<RefCell<Vec<Rc<RefCell<ObservedIds>>>>>,
        }
        impl Drop for ActiveObservationGuard {
            fn drop(&mut self) {
                self.stack.borrow_mut().pop();
            }
        }
        let _guard = ActiveObservationGuard {
            stack: Rc::clone(&self.active_read_targets),
        };

        let result = self.run_with_read_observer(block);

        if observed.borrow().is_empty() {
            if existing_entry.is_some() {
                self.clear(&scope);
            }
            return result;
        }

        let observed = {
            let mut observed = observed.borrow_mut();
            std::mem::replace(&mut *observed, ObservedIds::new())
        };
        let entry = existing_entry
            .unwrap_or_else(|| self.insert_scope_entry(scope.clone(), on_changed.clone()));
        {
            let mut entry_mut = entry.borrow_mut();
            entry_mut.update(scope, on_changed);
            entry_mut.last_seen_version = if has_frame_version {
                frame_version
            } else {
                u64::MAX
            };
        }
        self.replace_observed_ids(&entry, observed);

        result
    }

    fn with_no_observations<R>(&self, block: impl FnOnce() -> R) -> R {
        self.pause_count.set(self.pause_count.get() + 1);
        let result = block();
        self.pause_count
            .set(self.pause_count.get().saturating_sub(1));
        result
    }

    fn clear<T>(&self, scope: &T)
    where
        T: Any + PartialEq + 'static,
    {
        if let Some(rc_scope) = (scope as &dyn Any).downcast_ref::<RecomposeScope>() {
            if let Some(entry) = self.fast_scopes.borrow_mut().remove(&rc_scope.id()) {
                self.unregister_entry(&entry);
            }
            return;
        }

        let removed = {
            let mut owned_scopes = self.owned_scopes.borrow_mut();
            owned_scopes
                .iter()
                .position(|entry| entry.borrow().matches_scope(scope))
                .map(|index| owned_scopes.remove(index))
        };
        if let Some(entry) = removed {
            self.unregister_entry(&entry);
        }
    }

    fn clear_if(&self, predicate: impl Fn(&dyn Any) -> bool) {
        let removed_fast = {
            let mut fast_scopes = self.fast_scopes.borrow_mut();
            let removed_ids: Vec<_> = fast_scopes
                .iter()
                .filter(|(_, entry)| entry.borrow().matches_predicate(&predicate))
                .map(|(scope_id, _)| *scope_id)
                .collect();
            removed_ids
                .into_iter()
                .filter_map(|scope_id| fast_scopes.remove(&scope_id))
                .collect::<Vec<_>>()
        };
        let removed_owned = {
            let mut owned_scopes = self.owned_scopes.borrow_mut();
            let mut retained = Vec::with_capacity(owned_scopes.len());
            let mut removed = Vec::new();
            for entry in owned_scopes.drain(..) {
                if entry.borrow().matches_predicate(&predicate) {
                    removed.push(entry);
                } else {
                    retained.push(entry);
                }
            }
            *owned_scopes = retained;
            removed
        };

        for entry in removed_fast.into_iter().chain(removed_owned) {
            self.unregister_entry(&entry);
        }
    }

    fn clear_all(&self) {
        self.fast_scopes.borrow_mut().clear();
        self.owned_scopes.borrow_mut().clear();
        self.indexed_scopes.borrow_mut().clear();
        self.observed_to_scopes.borrow_mut().clear();
    }

    fn start(&self, weak_self: Weak<SnapshotStateObserverInner>) {
        if self.apply_handle.borrow().is_some() {
            return;
        }

        let handle = register_apply_observer(Rc::new(move |modified, _snapshot_id| {
            if let Some(inner) = weak_self.upgrade() {
                inner.handle_apply(modified);
            }
        }));
        self.apply_handle.replace(Some(handle));
    }

    fn stop(&self) {
        if let Some(handle) = self.apply_handle.borrow_mut().take() {
            drop(handle);
        }
    }

    fn find_scope_entry<T>(&self, scope: &T) -> Option<Rc<RefCell<ScopeEntry>>>
    where
        T: Any + PartialEq + 'static,
    {
        if let Some(scope) = (scope as &dyn Any).downcast_ref::<RecomposeScope>() {
            return self.fast_scopes.borrow().get(&scope.id()).cloned();
        }

        self.owned_scopes
            .borrow()
            .iter()
            .find(|entry| entry.borrow().matches_scope(scope))
            .cloned()
    }

    fn insert_scope_entry(
        &self,
        scope: impl Any + Clone + PartialEq + 'static,
        on_changed: Rc<dyn Fn(&dyn Any)>,
    ) -> Rc<RefCell<ScopeEntry>> {
        let entry_id = self.next_entry_id.get();
        self.next_entry_id.set(entry_id.wrapping_add(1));
        let recompose_scope_id = (&scope as &dyn Any)
            .downcast_ref::<RecomposeScope>()
            .map(RecomposeScope::id);
        let entry = Rc::new(RefCell::new(ScopeEntry::new(entry_id, scope, on_changed)));
        self.indexed_scopes
            .borrow_mut()
            .insert(entry_id, Rc::clone(&entry));
        if let Some(scope_id) = recompose_scope_id {
            self.fast_scopes
                .borrow_mut()
                .insert(scope_id, Rc::clone(&entry));
        } else {
            self.owned_scopes.borrow_mut().push(Rc::clone(&entry));
        }
        entry
    }

    fn prune_dead_scopes(&self) {
        let removed_fast = {
            let mut fast_scopes = self.fast_scopes.borrow_mut();
            let removed_ids: Vec<_> = fast_scopes
                .iter()
                .filter(|(_, entry)| !entry.borrow().should_retain())
                .map(|(scope_id, _)| *scope_id)
                .collect();
            let removed = removed_ids
                .into_iter()
                .filter_map(|scope_id| fast_scopes.remove(&scope_id))
                .collect::<Vec<_>>();
            shrink_map_if_sparse(&mut fast_scopes, Self::MIN_RETAINED_SCOPE_CAPACITY);
            removed
        };

        let removed_owned = {
            let mut owned_scopes = self.owned_scopes.borrow_mut();
            let mut retained = Vec::with_capacity(owned_scopes.len());
            let mut removed = Vec::new();
            for entry in owned_scopes.drain(..) {
                if entry.borrow().should_retain() {
                    retained.push(entry);
                } else {
                    removed.push(entry);
                }
            }
            *owned_scopes = retained;
            shrink_vec_if_sparse(&mut owned_scopes, Self::MIN_RETAINED_SCOPE_CAPACITY);
            removed
        };

        for entry in removed_fast.into_iter().chain(removed_owned) {
            self.unregister_entry(&entry);
        }
    }

    fn debug_stats(&self) -> SnapshotStateObserverDebugStats {
        let owned_scopes = self.owned_scopes.borrow();
        let fast_scopes = self.fast_scopes.borrow();
        let scopes_len = owned_scopes.len() + fast_scopes.len();
        let scopes_cap = owned_scopes.capacity() + fast_scopes.capacity();
        let mut observed_state_count = 0;
        let mut observed_state_capacity = 0;
        let mut stateless_scope_count = 0;

        for entry in owned_scopes.iter().chain(fast_scopes.values()) {
            let entry = entry.borrow();
            observed_state_count += entry.observed.len();
            observed_state_capacity += entry.observed.capacity();
            stateless_scope_count += usize::from(entry.observed.is_empty());
        }

        SnapshotStateObserverDebugStats {
            scopes_len,
            scopes_cap,
            fast_scopes_len: fast_scopes.len(),
            fast_scopes_cap: fast_scopes.capacity(),
            stateless_scope_count,
            observed_state_count,
            observed_state_capacity,
        }
    }

    fn run_with_read_observer<R>(&self, block: impl FnOnce() -> R) -> R {
        // Kotlin uses Snapshot.observeInternal which creates a TransparentObserverMutableSnapshot,
        // not a readonly snapshot. This allows writes to happen during observation (composition).
        use crate::snapshot_v2::take_transparent_observer_mutable_snapshot;

        // Create a transparent mutable snapshot (not readonly!) for observation
        // This matches Kotlin's Snapshot.observeInternal behavior
        let snapshot =
            take_transparent_observer_mutable_snapshot(Some(self.read_dispatcher.clone()), None);
        let result = snapshot.enter(block);
        snapshot.dispose();
        result
    }

    fn handle_apply(&self, modified: &[Arc<dyn StateObject>]) {
        if modified.is_empty() {
            return;
        }

        let mut seen_scope_ids = HashSet::default();
        let mut to_notify: Vec<Rc<RefCell<ScopeEntry>>> = Vec::new();
        {
            let observed_to_scopes = self.observed_to_scopes.borrow();
            let indexed_scopes = self.indexed_scopes.borrow();
            for state in modified {
                if let Some(scope_ids) = observed_to_scopes.get(&state.object_id().as_usize()) {
                    for &scope_id in scope_ids {
                        if seen_scope_ids.insert(scope_id) {
                            if let Some(entry) = indexed_scopes.get(&scope_id) {
                                to_notify.push(entry.clone());
                            }
                        }
                    }
                }
            }
        }

        if to_notify.is_empty() {
            return;
        }

        for entry in to_notify {
            let executor = self.executor.clone();
            executor(Box::new(move || {
                if let Ok(entry) = entry.try_borrow() {
                    entry.notify();
                }
            }));
        }
    }

    fn replace_observed_ids(&self, entry: &Rc<RefCell<ScopeEntry>>, observed: ObservedIds) {
        let (entry_id, previous) = {
            let mut entry_mut = entry.borrow_mut();
            let entry_id = entry_mut.id;
            let previous = std::mem::replace(&mut entry_mut.observed, observed);
            (entry_id, previous)
        };
        self.unregister_observed_ids(entry_id, &previous);
        let entry_ref = entry.borrow();
        self.register_observed_ids(entry_id, &entry_ref.observed);
    }

    fn register_observed_ids(&self, entry_id: usize, observed: &ObservedIds) {
        let mut observed_to_scopes = self.observed_to_scopes.borrow_mut();
        for &state_id in observed.iter() {
            let scope_ids = observed_to_scopes.entry(state_id).or_default();
            if !scope_ids.contains(&entry_id) {
                scope_ids.push(entry_id);
            }
        }
    }

    fn unregister_observed_ids(&self, entry_id: usize, observed: &ObservedIds) {
        let mut observed_to_scopes = self.observed_to_scopes.borrow_mut();
        let mut emptied = SmallVec::<[StateObjectId; MAX_OBSERVED_STATES]>::new();
        for &state_id in observed.iter() {
            if let Some(scope_ids) = observed_to_scopes.get_mut(&state_id) {
                scope_ids.retain(|tracked_id| *tracked_id != entry_id);
                if scope_ids.is_empty() {
                    emptied.push(state_id);
                }
            }
        }
        for state_id in emptied {
            observed_to_scopes.remove(&state_id);
        }
        shrink_map_if_sparse(&mut observed_to_scopes, Self::MIN_RETAINED_SCOPE_CAPACITY);
    }

    fn unregister_entry(&self, entry: &Rc<RefCell<ScopeEntry>>) {
        let (entry_id, observed) = {
            let mut entry_mut = entry.borrow_mut();
            let observed = std::mem::replace(&mut entry_mut.observed, ObservedIds::new());
            (entry_mut.id, observed)
        };
        self.unregister_observed_ids(entry_id, &observed);
        self.indexed_scopes.borrow_mut().remove(&entry_id);
    }
}
use smallvec::SmallVec;

fn shrink_map_if_sparse<K, V>(map: &mut HashMap<K, V>, min_retained_capacity: usize)
where
    K: Eq + std::hash::Hash,
{
    if map.capacity() <= map.len().max(min_retained_capacity).saturating_mul(4) {
        return;
    }

    let retained = map.len().max(min_retained_capacity);
    let mut rebuilt = HashMap::default();
    rebuilt.reserve(retained);
    rebuilt.extend(map.drain());
    *map = rebuilt;
}

fn shrink_vec_if_sparse<T>(items: &mut Vec<T>, min_retained_capacity: usize) {
    if items.capacity() <= items.len().max(min_retained_capacity).saturating_mul(4) {
        return;
    }

    let retained = items.len().max(min_retained_capacity);
    let mut rebuilt = Vec::with_capacity(retained);
    rebuilt.append(items);
    *items = rebuilt;
}

enum ObservedIds {
    Small(SmallVec<[StateObjectId; MAX_OBSERVED_STATES]>),
    Large(HashSet<StateObjectId>),
}

impl ObservedIds {
    fn new() -> Self {
        ObservedIds::Small(SmallVec::new())
    }

    fn insert(&mut self, id: StateObjectId) {
        match self {
            ObservedIds::Small(small) => {
                if small.contains(&id) {
                    return;
                }
                if small.len() < MAX_OBSERVED_STATES {
                    small.push(id);
                } else {
                    let mut large =
                        HashSet::with_capacity_and_hasher(small.len() + 1, Default::default());
                    for existing in small.iter() {
                        large.insert(*existing);
                    }
                    large.insert(id);
                    *self = ObservedIds::Large(large);
                }
            }
            ObservedIds::Large(large) => {
                large.insert(id);
            }
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            ObservedIds::Small(small) => small.is_empty(),
            ObservedIds::Large(large) => large.is_empty(),
        }
    }

    fn len(&self) -> usize {
        match self {
            ObservedIds::Small(small) => small.len(),
            ObservedIds::Large(large) => large.len(),
        }
    }

    fn capacity(&self) -> usize {
        match self {
            ObservedIds::Small(small) => small.capacity(),
            ObservedIds::Large(large) => large.capacity(),
        }
    }

    fn iter(&self) -> Box<dyn Iterator<Item = &StateObjectId> + '_> {
        match self {
            ObservedIds::Small(small) => Box::new(small.iter()),
            ObservedIds::Large(large) => Box::new(large.iter()),
        }
    }
}

// Most scopes observe only a handful of state objects. Spill into HashSet after that.
const MAX_OBSERVED_STATES: usize = 8;

enum ScopeStorage {
    Owned(Box<dyn Any>),
    RecomposeScope {
        id: ScopeId,
        weak: Weak<RecomposeScopeInner>,
    },
}

struct ScopeEntry {
    id: usize,
    scope: ScopeStorage,
    on_changed: Rc<dyn Fn(&dyn Any)>,
    observed: ObservedIds,
    last_seen_version: u64,
}

impl ScopeEntry {
    fn new<T>(id: usize, scope: T, on_changed: Rc<dyn Fn(&dyn Any)>) -> Self
    where
        T: Any + 'static,
    {
        Self {
            id,
            scope: ScopeStorage::from_value(scope),
            on_changed,
            observed: ObservedIds::new(),
            last_seen_version: u64::MAX,
        }
    }

    fn update<T>(&mut self, new_scope: T, on_changed: Rc<dyn Fn(&dyn Any)>)
    where
        T: Any + 'static,
    {
        self.scope = ScopeStorage::from_value(new_scope);
        self.on_changed = on_changed;
    }

    fn matches_scope<T>(&self, scope: &T) -> bool
    where
        T: Any + PartialEq + 'static,
    {
        if let Some(scope) = (scope as &dyn Any).downcast_ref::<RecomposeScope>() {
            return matches!(
                &self.scope,
                ScopeStorage::RecomposeScope { id, .. } if *id == scope.id()
            );
        }

        match &self.scope {
            ScopeStorage::Owned(stored) => stored
                .downcast_ref::<T>()
                .map(|stored| stored == scope)
                .unwrap_or(false),
            ScopeStorage::RecomposeScope { .. } => false,
        }
    }

    fn matches_predicate(&self, predicate: &impl Fn(&dyn Any) -> bool) -> bool {
        match &self.scope {
            ScopeStorage::Owned(scope) => predicate(scope.as_ref()),
            ScopeStorage::RecomposeScope { weak, .. } => weak
                .upgrade()
                .map(|inner| predicate(&RecomposeScope { inner }))
                .unwrap_or(true),
        }
    }

    fn should_retain(&self) -> bool {
        match &self.scope {
            ScopeStorage::Owned(_) => true,
            ScopeStorage::RecomposeScope { weak, .. } => weak.upgrade().is_some(),
        }
    }

    fn notify(&self) {
        match &self.scope {
            ScopeStorage::Owned(scope) => (self.on_changed)(scope.as_ref()),
            ScopeStorage::RecomposeScope { weak, .. } => {
                if let Some(inner) = weak.upgrade() {
                    (self.on_changed)(&RecomposeScope { inner });
                }
            }
        }
    }
}

impl ScopeStorage {
    fn from_value<T>(value: T) -> Self
    where
        T: Any + 'static,
    {
        let any = &value as &dyn Any;
        if let Some(scope) = any.downcast_ref::<RecomposeScope>() {
            Self::RecomposeScope {
                id: scope.id(),
                weak: scope.downgrade(),
            }
        } else {
            Self::Owned(Box::new(value))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot_v2::take_mutable_snapshot;
    use crate::snapshot_v2::{reset_runtime_for_tests, TestRuntimeGuard};
    use crate::state::{NeverEqual, SnapshotMutableState};
    use std::cell::Cell;

    fn reset_runtime() -> TestRuntimeGuard {
        reset_runtime_for_tests()
    }

    #[derive(Clone, PartialEq)]
    struct TestScope(&'static str);

    #[test]
    fn notifies_scope_when_state_changes() {
        let _guard = reset_runtime();

        let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
        let triggered = Rc::new(Cell::new(0));
        let observer_trigger = triggered.clone();

        let observer = SnapshotStateObserver::new(|callback| callback());
        observer.start();

        let scope = TestScope("scope");
        observer.observe_reads(
            scope.clone(),
            move |_| {
                observer_trigger.set(observer_trigger.get() + 1);
            },
            || {
                let _ = state.get();
            },
        );

        let snapshot = take_mutable_snapshot(None, None);
        snapshot.enter(|| {
            state.set(1);
        });
        snapshot.apply().check();

        assert_eq!(triggered.get(), 1);
        observer.stop();
    }

    #[test]
    fn clear_removes_scope_observation() {
        let _guard = reset_runtime();

        let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
        let triggered = Rc::new(Cell::new(0));
        let observer_trigger = triggered.clone();

        let observer = SnapshotStateObserver::new(|callback| callback());
        observer.start();

        let scope = TestScope("scope");
        observer.observe_reads(
            scope.clone(),
            move |_| {
                observer_trigger.set(observer_trigger.get() + 1);
            },
            || {
                let _ = state.get();
            },
        );

        observer.clear(&scope);

        let snapshot = take_mutable_snapshot(None, None);
        snapshot.enter(|| {
            state.set(1);
        });
        snapshot.apply().check();

        assert_eq!(triggered.get(), 0);
        observer.stop();
    }

    #[test]
    fn with_no_observations_skips_reads() {
        let _guard = reset_runtime();

        let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
        let triggered = Rc::new(Cell::new(0));
        let observer_trigger = triggered.clone();

        let observer = SnapshotStateObserver::new(|callback| callback());
        observer.start();

        let scope = TestScope("scope");
        observer.observe_reads(
            scope.clone(),
            move |_| {
                observer_trigger.set(observer_trigger.get() + 1);
            },
            || {
                observer.with_no_observations(|| {
                    let _ = state.get();
                });
            },
        );

        let snapshot = take_mutable_snapshot(None, None);
        snapshot.enter(|| {
            state.set(1);
        });
        snapshot.apply().check();

        assert_eq!(triggered.get(), 0);
        observer.stop();
    }

    #[test]
    fn nested_observe_reads_attributes_state_to_innermost_scope_only() {
        let _guard = reset_runtime();

        let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
        let outer_triggered = Rc::new(Cell::new(0));
        let inner_triggered = Rc::new(Cell::new(0));

        let observer = SnapshotStateObserver::new(|callback| callback());
        observer.start();

        let outer_scope = TestScope("outer");
        let inner_scope = TestScope("inner");
        observer.observe_reads(
            outer_scope.clone(),
            {
                let outer_triggered = Rc::clone(&outer_triggered);
                move |_| outer_triggered.set(outer_triggered.get() + 1)
            },
            || {
                observer.observe_reads(
                    inner_scope.clone(),
                    {
                        let inner_triggered = Rc::clone(&inner_triggered);
                        move |_| inner_triggered.set(inner_triggered.get() + 1)
                    },
                    || {
                        let _ = state.get();
                    },
                );
            },
        );

        let snapshot = take_mutable_snapshot(None, None);
        snapshot.enter(|| {
            state.set(1);
        });
        snapshot.apply().check();

        assert_eq!(outer_triggered.get(), 0);
        assert_eq!(inner_triggered.get(), 1);
        observer.stop();
    }

    #[test]
    fn stateless_recompose_scope_does_not_retain_observer_entry() {
        let _guard = reset_runtime();

        let observer = SnapshotStateObserver::new(|callback| callback());
        let runtime = crate::TestRuntime::new();
        let scope = RecomposeScope::new_for_test(runtime.handle());

        observer.observe_reads(scope, |_| {}, || {});

        let stats = observer.debug_stats();
        assert_eq!(stats.scopes_len, 0);
        assert_eq!(stats.fast_scopes_len, 0);
        assert_eq!(stats.stateless_scope_count, 0);
    }

    #[test]
    fn scope_that_stops_reading_state_is_removed_immediately() {
        let _guard = reset_runtime();

        let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
        let observer = SnapshotStateObserver::new(|callback| callback());
        let runtime = crate::TestRuntime::new();
        let scope = RecomposeScope::new_for_test(runtime.handle());
        let triggered = Rc::new(Cell::new(0));
        let observer_trigger = Rc::clone(&triggered);

        observer.observe_reads(
            scope.clone(),
            move |_| observer_trigger.set(observer_trigger.get() + 1),
            || {
                let _ = state.get();
            },
        );

        let after_stateful = observer.debug_stats();
        assert_eq!(after_stateful.scopes_len, 1);
        assert_eq!(after_stateful.fast_scopes_len, 1);

        observer.observe_reads(scope, |_| {}, || {});

        let after_stateless = observer.debug_stats();
        assert_eq!(after_stateless.scopes_len, 0);
        assert_eq!(after_stateless.fast_scopes_len, 0);

        let snapshot = take_mutable_snapshot(None, None);
        snapshot.enter(|| {
            state.set(1);
        });
        snapshot.apply().check();

        assert_eq!(triggered.get(), 0);
    }

    #[test]
    fn begin_frame_prunes_dropped_recompose_scope_entries() {
        let _guard = reset_runtime();

        let state = SnapshotMutableState::new_in_arc(0, Arc::new(NeverEqual));
        let observer = SnapshotStateObserver::new(|callback| callback());
        let runtime = crate::TestRuntime::new();
        let scope = RecomposeScope::new_for_test(runtime.handle());

        observer.observe_reads(
            scope.clone(),
            |_| {},
            || {
                let _ = state.get();
            },
        );

        assert_eq!(observer.inner.owned_scopes.borrow().len(), 0);
        assert_eq!(observer.inner.fast_scopes.borrow().len(), 1);

        drop(scope);
        observer.begin_frame();

        assert_eq!(observer.inner.owned_scopes.borrow().len(), 0);
        assert_eq!(observer.inner.fast_scopes.borrow().len(), 0);
    }
}

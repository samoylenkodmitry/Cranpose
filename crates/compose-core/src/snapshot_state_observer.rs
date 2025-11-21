// Observer callbacks use Arc for shared ownership but may capture non-Send types.
// This is safe because callbacks are always invoked on the UI thread where they were created.
#![allow(clippy::arc_with_non_send_sync)]
// Complex types are inherent to the observer pattern with nested callbacks and state tracking
#![allow(clippy::type_complexity)]

use crate::collections::map::HashSet;
use crate::snapshot_v2::{register_apply_observer, ReadObserver, StateObjectId};
use crate::state::StateObject;
use ahash::HashSetExt;
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
/// needed by the Compose-RS runtime:
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

    /// Test-only helper to simulate snapshot changes.
    #[cfg(test)]
    pub fn notify_changes(&self, modified: &[Arc<dyn StateObject>]) {
        self.inner.handle_apply(modified);
    }
}

struct SnapshotStateObserverInner {
    executor: Rc<Executor>,
    scopes: RefCell<Vec<Rc<RefCell<ScopeEntry>>>>,
    fast_scopes: RefCell<Vec<Option<Rc<RefCell<ScopeEntry>>>>>,
    pause_count: Rc<Cell<usize>>,
    apply_handle: RefCell<Option<crate::snapshot_v2::ObserverHandle>>,
    weak_self: RefCell<Weak<SnapshotStateObserverInner>>,
    frame_version: Cell<u64>,
}

impl SnapshotStateObserverInner {
    fn new(on_changed_executor: impl Fn(Box<dyn FnOnce() + 'static>) + 'static) -> Self {
        Self {
            executor: Rc::new(on_changed_executor),
            scopes: RefCell::new(Vec::new()),
            fast_scopes: RefCell::new(Vec::new()),
            pause_count: Rc::new(Cell::new(0)),
            apply_handle: RefCell::new(None),
            weak_self: RefCell::new(Weak::new()),
            frame_version: Cell::new(0),
        }
    }

    fn set_self(&self, weak: Weak<SnapshotStateObserverInner>) {
        self.weak_self.replace(weak);
    }

    fn begin_frame(&self) {
        let next = self.frame_version.get().wrapping_add(1);
        self.frame_version.set(next);
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

        let entry = self.get_scope_entry(scope.clone(), on_changed.clone());

        let pause_count = self.pause_count.clone();

        let read_observer: ReadObserver = {
            let mut entry_mut = entry.borrow_mut();
            entry_mut.update(scope, on_changed);

            let already_observed =
                has_frame_version && entry_mut.last_seen_version == frame_version;
            if already_observed || entry_mut.is_stateless {
                drop(entry_mut);
                return block();
            }

            entry_mut.observed.clear();
            entry_mut.last_seen_version = if has_frame_version {
                frame_version
            } else {
                u64::MAX
            };
            entry_mut.is_stateless = false;

            if let Some(observer) = entry_mut.read_observer.clone() {
                observer
            } else {
                let entry_for_observer = entry.clone();
                let pause_count = pause_count.clone();

                let observer: ReadObserver = Arc::new(move |state| {
                    if pause_count.get() > 0 {
                        return;
                    }
                    let mut entry_ref = entry_for_observer.borrow_mut();
                    let id = state.object_id().as_usize();
                    entry_ref.observed.insert(id);
                    entry_ref.is_stateless = false;
                });

                entry_mut.read_observer = Some(observer.clone());
                observer
            }
        };

        let result = self.run_with_read_observer(read_observer, block);

        {
            let mut entry_mut = entry.borrow_mut();
            if entry_mut.observed.is_empty() {
                entry_mut.is_stateless = true;
            }
        }

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
        self.scopes
            .borrow_mut()
            .retain(|entry| !entry.borrow().matches_scope(scope));
    }

    fn clear_if(&self, predicate: impl Fn(&dyn Any) -> bool) {
        self.scopes.borrow_mut().retain(|entry| {
            let entry_ref = entry.borrow();
            !predicate(entry_ref.scope())
        });
    }

    fn clear_all(&self) {
        self.scopes.borrow_mut().clear();
    }

    // Arc-wrapped closure captures Weak which may not be Send/Sync. This is safe because
    // the observer callback is only invoked on the UI thread where it was registered.
    #[allow(clippy::arc_with_non_send_sync)]
    fn start(&self, weak_self: Weak<SnapshotStateObserverInner>) {
        if self.apply_handle.borrow().is_some() {
            return;
        }

        let handle = register_apply_observer(Arc::new(move |modified, _snapshot_id| {
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

    fn get_scope_entry(
        &self,
        scope: impl Any + Clone + PartialEq + 'static,
        on_changed: Rc<dyn Fn(&dyn Any)>,
    ) -> Rc<RefCell<ScopeEntry>> {
        // ---------- FAST PATH: real compose scope ----------
        if let Some(rc_scope) = (&scope as &dyn Any).downcast_ref::<RecomposeScope>() {
            let id: usize = rc_scope.id(); // or `.0` or similar

            let mut fast = self.fast_scopes.borrow_mut();

            if id >= fast.len() {
                fast.resize_with(id + 1, || None);
            }

            if let Some(existing) = &fast[id] {
                return existing.clone();
            }

            let entry = Rc::new(RefCell::new(ScopeEntry::new(scope, on_changed)));
            fast[id] = Some(entry.clone());
            return entry;
        }

        // ---------- SLOW / GENERIC PATH ----------
        let mut scopes = self.scopes.borrow_mut();

        if let Some(existing) = scopes
            .iter()
            .find(|entry| entry.borrow().matches_scope(&scope))
        {
            return existing.clone();
        }

        let entry = Rc::new(RefCell::new(ScopeEntry::new(scope, on_changed)));
        scopes.push(entry.clone());
        entry
    }

    fn run_with_read_observer<R>(
        &self,
        read_observer: ReadObserver,
        block: impl FnOnce() -> R,
    ) -> R {
        // Kotlin uses Snapshot.observeInternal which creates a TransparentObserverMutableSnapshot,
        // not a readonly snapshot. This allows writes to happen during observation (composition).
        use crate::snapshot_v2::take_transparent_observer_mutable_snapshot;

        // Create a transparent mutable snapshot (not readonly!) for observation
        // This matches Kotlin's Snapshot.observeInternal behavior
        let snapshot = take_transparent_observer_mutable_snapshot(Some(read_observer), None);
        let result = snapshot.enter(block);
        snapshot.dispose();
        result
    }

    fn handle_apply(&self, modified: &[Arc<dyn StateObject>]) {
        if modified.is_empty() {
            return;
        }

        let mut modified_ids: SmallVec<usize, MAX_OBSERVED_STATES> = SmallVec::new();
        for state in modified {
            modified_ids.push(state.object_id().as_usize());
        }

        let scopes = self.scopes.borrow();
        let mut to_notify: Vec<Rc<RefCell<ScopeEntry>>> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::default();

        for entry in scopes.iter() {
            let entry_ref = entry.borrow();
            if entry_ref
                .observed
                .iter()
                .any(|id| modified_ids.contains(id))
            {
                let ptr = Rc::as_ptr(entry) as usize;
                if seen.insert(ptr) {
                    to_notify.push(entry.clone());
                }
            }
        }
        drop(scopes);

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
}

use compose_core::RecomposeScope;
use smallvec::SmallVec;

enum ObservedIds {
    Small(SmallVec<StateObjectId, MAX_OBSERVED_STATES>),
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
                    let mut large = HashSet::with_capacity(small.len() + 1);
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

    fn clear(&mut self) {
        match self {
            ObservedIds::Small(small) => small.clear(),
            ObservedIds::Large(large) => large.clear(),
        }
    }

    fn iter(&self) -> Box<dyn Iterator<Item = &StateObjectId> + '_> {
        match self {
            ObservedIds::Small(small) => Box::new(small.iter()),
            ObservedIds::Large(large) => Box::new(large.iter()),
        }
    }
}

const MAX_OBSERVED_STATES: usize = 8;
struct ScopeEntry {
    scope: Box<dyn Any>,
    on_changed: Rc<dyn Fn(&dyn Any)>,
    observed: ObservedIds,
    read_observer: Option<ReadObserver>,
    is_stateless: bool,
    last_seen_version: u64,
}

impl ScopeEntry {
    fn new<T>(scope: T, on_changed: Rc<dyn Fn(&dyn Any)>) -> Self
    where
        T: Any + 'static,
    {
        Self {
            scope: Box::new(scope),
            on_changed,
            observed: ObservedIds::new(),
            read_observer: None,
            is_stateless: false,
            last_seen_version: u64::MAX,
        }
    }

    fn update<T>(&mut self, new_scope: T, on_changed: Rc<dyn Fn(&dyn Any)>)
    where
        T: Any + 'static,
    {
        self.scope = Box::new(new_scope);
        self.on_changed = on_changed;
    }

    fn matches_scope<T>(&self, scope: &T) -> bool
    where
        T: Any + PartialEq + 'static,
    {
        self.scope
            .downcast_ref::<T>()
            .map(|stored| stored == scope)
            .unwrap_or(false)
    }

    fn scope(&self) -> &dyn Any {
        &*self.scope
    }

    fn notify(&self) {
        (self.on_changed)(self.scope());
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
}

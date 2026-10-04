use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

use smallvec::SmallVec;

use crate::{
    collections::map::{HashMap, HashSet},
    hash::default as default_hash,
    snapshot_v2::{
        ReadObserver, StateObjectId, TransparentObserverMutableSnapshot, register_apply_observer,
    },
    state::StateObject,
};

type Executor = dyn Fn(Box<dyn FnOnce() + 'static>) + 'static;

trait ScopeChangedCallback: Fn(&dyn Any) + Any {}

impl<F: Fn(&dyn Any) + Any> ScopeChangedCallback for F {}

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
    pub stateless_scope_count: usize,
    pub observed_state_count: usize,
    pub observed_state_capacity: usize,
}

impl SnapshotStateObserver {
    /// Create a new observer that schedules callbacks using `on_changed_executor`.
    pub fn new(on_changed_executor: impl Fn(Box<dyn FnOnce() + 'static>) + 'static) -> Self {
        Self {
            inner: Rc::new(SnapshotStateObserverInner::new(on_changed_executor)),
        }
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
        T: Any + Clone + Eq + Hash + 'static,
    {
        self.inner
            .observe_reads(scope, on_value_changed_for_scope, block)
    }

    /// Temporarily pause read observation while executing `block`.
    pub fn with_no_observations<R>(&self, block: impl FnOnce() -> R) -> R {
        self.inner.with_no_observations(block)
    }

    /// Remove any recorded reads for `scope`.
    pub fn clear<T>(&self, scope: &T)
    where
        T: Any + Eq + Hash + 'static,
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

    #[cfg(test)]
    pub fn notify_changes(&self, modified: &[Rc<dyn StateObject>]) {
        self.inner.handle_apply(modified);
    }
}

struct SnapshotStateObserverInner {
    executor: Rc<Executor>,
    owned_scopes: RefCell<HashMap<OwnedScopeIndexKey, OwnedScopeBucket>>,
    indexed_scopes: RefCell<HashMap<usize, Rc<RefCell<ScopeEntry>>>>,
    observed_to_scopes: RefCell<HashMap<StateObjectId, HashSet<usize>>>,
    pause_count: Rc<Cell<usize>>,
    active_read_targets: Rc<RefCell<ReadObservationStack>>,
    read_dispatcher: ReadObserver,
    read_snapshot: RefCell<Option<Rc<TransparentObserverMutableSnapshot>>>,
    apply_handle: RefCell<Option<crate::snapshot_v2::ObserverHandle>>,
    next_entry_id: Cell<usize>,
    /// One `Rc` per type of callback that captures nothing: see
    /// [`SnapshotStateObserverInner::capture_free_callback`].
    capture_free_callbacks: RefCell<CaptureFreeCallbacks>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct OwnedScopeIndexKey {
    type_id: TypeId,
    value_hash: u64,
}

type OwnedScopeBucket = SmallVec<[Rc<RefCell<ScopeEntry>>; 1]>;

/// The shared `Rc` of each type of callback that captures nothing.
type CaptureFreeCallbacks = SmallVec<[(TypeId, Rc<dyn ScopeChangedCallback>); 2]>;

fn owned_scope_index_key<T>(scope: &T) -> OwnedScopeIndexKey
where
    T: Any + Hash + 'static,
{
    let mut hasher = default_hash::new();
    scope.hash(&mut hasher);
    OwnedScopeIndexKey {
        type_id: TypeId::of::<T>(),
        value_hash: hasher.finish(),
    }
}

impl SnapshotStateObserverInner {
    const MIN_RETAINED_SCOPE_CAPACITY: usize = 256;

    fn new(on_changed_executor: impl Fn(Box<dyn FnOnce() + 'static>) + 'static) -> Self {
        let pause_count = Rc::new(Cell::new(0));
        let active_read_targets = Rc::new(RefCell::new(ReadObservationStack::default()));
        let dispatcher_pause_count = Rc::clone(&pause_count);
        let dispatcher_targets = Rc::clone(&active_read_targets);
        let read_dispatcher: ReadObserver = Rc::new(move |state| {
            if dispatcher_pause_count.get() > 0 {
                return;
            }
            let observed = dispatcher_targets.borrow().last().cloned();
            if let Some(observed) = observed {
                observed.borrow_mut().insert(state);
            }
        });

        Self {
            executor: Rc::new(on_changed_executor),
            owned_scopes: RefCell::new(HashMap::default()),
            indexed_scopes: RefCell::new(HashMap::default()),
            observed_to_scopes: RefCell::new(HashMap::default()),
            pause_count,
            active_read_targets,
            read_dispatcher,
            read_snapshot: RefCell::new(None),
            apply_handle: RefCell::new(None),
            next_entry_id: Cell::new(0),
            capture_free_callbacks: RefCell::new(SmallVec::new()),
        }
    }

    fn observe_reads<T, R>(
        &self,
        scope: T,
        on_value_changed_for_scope: impl Fn(&T) + 'static,
        block: impl FnOnce() -> R,
    ) -> R
    where
        T: Any + Clone + Eq + Hash + 'static,
    {
        let existing_entry = self.find_scope_entry(&scope);
        let on_changed = std::cell::LazyCell::new(|| {
            let callback = move |scope_any: &dyn Any| {
                if let Some(typed) = scope_any.downcast_ref::<T>() {
                    on_value_changed_for_scope(typed);
                }
            };
            if std::mem::size_of_val(&callback) == 0 {
                return self.capture_free_callback(callback);
            }
            match existing_entry.as_ref() {
                Some(entry) => entry.borrow_mut().callback_reusing(callback),
                None => Rc::new(callback),
            }
        });

        if let Some(entry) = existing_entry.as_ref() {
            entry.borrow_mut().update_scope(&scope);
            let callback = on_changed.clone();
            entry.borrow_mut().on_changed = callback;
        }

        let observed = self.active_read_targets.borrow_mut().push();
        struct ActiveObservationGuard<'a> {
            stack: &'a RefCell<ReadObservationStack>,
        }
        impl Drop for ActiveObservationGuard<'_> {
            fn drop(&mut self) {
                let target = self.stack.borrow_mut().pop();
                target.borrow_mut().clear();
            }
        }
        let _guard = ActiveObservationGuard {
            stack: &self.active_read_targets,
        };

        let result = self.run_with_read_observer(block);

        if observed.borrow().is_empty() {
            if existing_entry.is_some() {
                self.clear(&scope);
            }
            return result;
        }

        let callback = Rc::clone(&on_changed);
        drop(on_changed);
        let entry = existing_entry
            .unwrap_or_else(|| self.insert_scope_entry(scope.clone(), Rc::clone(&callback)));
        entry.borrow_mut().update(&scope, callback);
        self.replace_observed_ids(&entry, &mut observed.borrow_mut());

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
        T: Any + Eq + Hash + 'static,
    {
        let removed = self.remove_scope_entry(scope);
        if let Some(entry) = removed {
            self.unregister_entry(&entry);
        }
    }

    fn clear_if(&self, predicate: impl Fn(&dyn Any) -> bool) {
        let removed = self.partition_scopes(|entry| predicate(entry.scope.as_ref()));
        for entry in removed {
            self.unregister_entry(&entry);
        }
    }

    fn clear_all(&self) {
        let entries = std::mem::take(&mut *self.indexed_scopes.borrow_mut());
        let owned = std::mem::take(&mut *self.owned_scopes.borrow_mut());
        self.observed_to_scopes.borrow_mut().clear();
        drop((entries, owned));
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

    /// The `Rc` of a callback that captures nothing. Every closure of such a
    /// type does the same thing, so one `Rc` serves all its scopes instead
    /// of one each.
    fn capture_free_callback<F: Fn(&dyn Any) + 'static>(
        &self,
        callback: F,
    ) -> Rc<dyn ScopeChangedCallback> {
        let type_id = TypeId::of::<F>();
        let mut shared = self.capture_free_callbacks.borrow_mut();
        if let Some((_, callback)) = shared.iter().find(|(id, _)| *id == type_id) {
            return Rc::clone(callback);
        }
        let callback: Rc<dyn ScopeChangedCallback> = Rc::new(callback);
        shared.push((type_id, Rc::clone(&callback)));
        callback
    }

    fn insert_scope_entry(
        &self,
        scope: impl Any + Clone + Eq + Hash + 'static,
        on_changed: Rc<dyn ScopeChangedCallback>,
    ) -> Rc<RefCell<ScopeEntry>> {
        let entry_id = self.next_entry_id.get();
        self.next_entry_id.set(entry_id.wrapping_add(1));
        let scope_key = owned_scope_index_key(&scope);
        let entry = Rc::new(RefCell::new(ScopeEntry::new(entry_id, scope, on_changed)));
        self.indexed_scopes
            .borrow_mut()
            .insert(entry_id, Rc::clone(&entry));
        self.owned_scopes
            .borrow_mut()
            .entry(scope_key)
            .or_default()
            .push(Rc::clone(&entry));
        entry
    }

    fn find_scope_entry<T>(&self, scope: &T) -> Option<Rc<RefCell<ScopeEntry>>>
    where
        T: Any + Eq + Hash + 'static,
    {
        let key = owned_scope_index_key(scope);
        self.owned_scopes.borrow().get(&key).and_then(|bucket| {
            bucket
                .iter()
                .find(|entry| entry.borrow().matches_scope(scope))
                .cloned()
        })
    }

    fn remove_scope_entry<T>(&self, scope: &T) -> Option<Rc<RefCell<ScopeEntry>>>
    where
        T: Any + Eq + Hash + 'static,
    {
        let key = owned_scope_index_key(scope);
        let mut owned_scopes = self.owned_scopes.borrow_mut();
        let mut removed = None;
        let mut remove_bucket = false;
        if let Some(bucket) = owned_scopes.get_mut(&key)
            && let Some(index) = bucket
                .iter()
                .position(|entry| entry.borrow().matches_scope(scope))
        {
            removed = Some(bucket.remove(index));
            remove_bucket = bucket.is_empty();
        }
        if remove_bucket {
            owned_scopes.remove(&key);
        }
        shrink_map_if_sparse(&mut owned_scopes, Self::MIN_RETAINED_SCOPE_CAPACITY);
        removed
    }

    fn partition_scopes(
        &self,
        should_remove: impl Fn(&ScopeEntry) -> bool,
    ) -> Vec<Rc<RefCell<ScopeEntry>>> {
        let mut owned_scopes = self.owned_scopes.borrow_mut();
        let mut removed = Vec::new();
        owned_scopes.retain(|_, bucket| {
            let mut index = 0;
            while index < bucket.len() {
                let remove = should_remove(&bucket[index].borrow());
                if remove {
                    removed.push(bucket.swap_remove(index));
                } else {
                    index += 1;
                }
            }
            !bucket.is_empty()
        });
        shrink_map_if_sparse(&mut owned_scopes, Self::MIN_RETAINED_SCOPE_CAPACITY);
        removed
    }

    fn debug_stats(&self) -> SnapshotStateObserverDebugStats {
        let owned_scopes = self.owned_scopes.borrow();
        let indexed_scopes = self.indexed_scopes.borrow();
        let owned_scope_cap =
            owned_scopes.capacity() + owned_scopes.values().map(SmallVec::capacity).sum::<usize>();
        let scopes_cap = owned_scope_cap + indexed_scopes.capacity();
        let mut observed_state_count = 0;
        let mut observed_state_capacity = 0;
        let mut stateless_scope_count = 0;

        for entry in indexed_scopes.values() {
            let entry = entry.borrow();
            observed_state_count += entry.observed.len();
            observed_state_capacity += entry.observed.capacity();
            stateless_scope_count += usize::from(entry.observed.is_empty());
        }

        SnapshotStateObserverDebugStats {
            scopes_len: indexed_scopes.len(),
            scopes_cap,
            stateless_scope_count,
            observed_state_count,
            observed_state_capacity,
        }
    }

    fn run_with_read_observer<R>(&self, block: impl FnOnce() -> R) -> R {
        use crate::snapshot_v2::{
            current_snapshot_reads_into, take_transparent_observer_mutable_snapshot_reusing,
        };

        if current_snapshot_reads_into(&self.read_dispatcher) {
            return block();
        }

        let mut snapshot = take_transparent_observer_mutable_snapshot_reusing(
            Some(self.read_dispatcher.clone()),
            None,
            self.read_snapshot.take(),
        );
        let result = snapshot.enter(block);
        snapshot.dispose();
        if Rc::get_mut(&mut snapshot).is_some() && !snapshot.has_pending_changes() {
            self.read_snapshot.replace(Some(snapshot));
        }
        result
    }

    fn handle_apply(&self, modified: &[Rc<dyn StateObject>]) {
        if modified.is_empty() {
            return;
        }

        let mut seen_scope_ids: HashSet<usize> = HashSet::default();
        let mut to_notify: Vec<Rc<RefCell<ScopeEntry>>> = Vec::new();
        {
            let observed_to_scopes = self.observed_to_scopes.borrow();
            let indexed_scopes = self.indexed_scopes.borrow();
            for state in modified {
                if let Some(scope_ids) = observed_to_scopes.get(&state.object_id().as_usize()) {
                    let mut ordered_scope_ids: SmallVec<[usize; 8]> =
                        scope_ids.iter().copied().collect();
                    ordered_scope_ids.sort_unstable();
                    for scope_id in ordered_scope_ids {
                        if seen_scope_ids.insert(scope_id)
                            && let Some(entry) = indexed_scopes.get(&scope_id)
                        {
                            to_notify.push(entry.clone());
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

    fn replace_observed_ids(&self, entry: &Rc<RefCell<ScopeEntry>>, collected: &mut ObservedIds) {
        let (entry_id, previous) = {
            let mut entry_mut = entry.borrow_mut();
            if entry_mut.observed.iter().eq(collected.iter()) {
                entry_mut.observed.take_leases(collected);
                collected.clear();
                return;
            }
            let entry_id = entry_mut.id;
            let previous = std::mem::replace(&mut entry_mut.observed, collected.take_sized());
            (entry_id, previous)
        };
        let entry_ref = entry.borrow();
        self.unregister_observed_ids(entry_id, &previous);
        self.register_observed_ids(entry_id, &entry_ref.observed);
    }

    fn register_observed_ids(&self, entry_id: usize, observed: &ObservedIds) {
        let mut observed_to_scopes = self.observed_to_scopes.borrow_mut();
        for state_id in observed.iter() {
            let scope_ids = observed_to_scopes.entry(state_id).or_default();
            scope_ids.insert(entry_id);
        }
    }

    fn unregister_observed_ids(&self, entry_id: usize, observed: &ObservedIds) {
        let mut observed_to_scopes = self.observed_to_scopes.borrow_mut();
        let mut emptied = SmallVec::<[StateObjectId; MAX_OBSERVED_STATES]>::new();
        for state_id in observed.iter() {
            if let Some(scope_ids) = observed_to_scopes.get_mut(&state_id) {
                scope_ids.remove(&entry_id);
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

#[derive(Default)]
struct ReadObservationStack {
    targets: Vec<Rc<RefCell<ObservedIds>>>,
    depth: usize,
}

impl ReadObservationStack {
    fn push(&mut self) -> Rc<RefCell<ObservedIds>> {
        if self.depth == self.targets.len() {
            self.targets.push(Rc::new(RefCell::new(ObservedIds::new())));
        }
        let target = Rc::clone(&self.targets[self.depth]);
        self.depth += 1;
        target
    }

    fn last(&self) -> Option<&Rc<RefCell<ObservedIds>>> {
        self.depth.checked_sub(1).map(|index| &self.targets[index])
    }

    fn pop(&mut self) -> Rc<RefCell<ObservedIds>> {
        self.depth -= 1;
        Rc::clone(&self.targets[self.depth])
    }
}

enum ObservedIds {
    Small(SmallVec<[ObservedState; 1]>),
    Large(Box<HashMap<StateObjectId, Option<Rc<dyn Any>>>>),
}

struct ObservedState {
    id: StateObjectId,
    _lease: Option<Rc<dyn Any>>,
}

impl ObservedIds {
    fn new() -> Self {
        ObservedIds::Small(SmallVec::new())
    }

    fn insert(&mut self, state: &dyn StateObject) {
        let id = state.object_id().as_usize();
        match self {
            ObservedIds::Small(small) => {
                if small.iter().any(|observed| observed.id == id) {
                    return;
                }
                if small.len() < MAX_OBSERVED_STATES {
                    small.push(ObservedState {
                        id,
                        _lease: state.observation_lease(),
                    });
                } else {
                    let mut large =
                        HashMap::with_capacity_and_hasher(small.len() + 1, Default::default());
                    for observed in small.drain(..) {
                        large.insert(observed.id, observed._lease);
                    }
                    large.insert(id, state.observation_lease());
                    *self = ObservedIds::Large(Box::new(large));
                }
            }
            ObservedIds::Large(large) => {
                large.entry(id).or_insert_with(|| state.observation_lease());
            }
        }
    }

    fn clear(&mut self) {
        match self {
            ObservedIds::Small(small) => small.clear(),
            ObservedIds::Large(_) => *self = ObservedIds::new(),
        }
    }

    /// Keeps `fresh`'s observation leases for the states both name, leaving
    /// the old ones in `fresh` to drop. An id is a state's address, which a
    /// new state can take after the old one is dropped, and only its own
    /// lease keeps the new one observed.
    fn take_leases(&mut self, fresh: &mut ObservedIds) {
        match (self, fresh) {
            (ObservedIds::Small(kept), ObservedIds::Small(fresh)) => {
                for (kept, fresh) in kept.iter_mut().zip(fresh.iter_mut()) {
                    std::mem::swap(&mut kept._lease, &mut fresh._lease);
                }
            }
            (kept, fresh) => std::mem::swap(kept, fresh),
        }
    }

    fn take_sized(&mut self) -> ObservedIds {
        match self {
            ObservedIds::Small(small) => ObservedIds::Small(small.drain(..).collect()),
            ObservedIds::Large(_) => std::mem::replace(self, ObservedIds::new()),
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

    fn iter(&self) -> impl Iterator<Item = StateObjectId> + '_ {
        let (small, large) = match self {
            ObservedIds::Small(small) => (Some(small.as_slice()), None),
            ObservedIds::Large(large) => (None, Some(large)),
        };
        small
            .into_iter()
            .flatten()
            .map(|observed| observed.id)
            .chain(large.into_iter().flat_map(|states| states.keys().copied()))
    }
}

const MAX_OBSERVED_STATES: usize = 8;

struct ScopeEntry {
    id: usize,
    scope: Box<dyn Any>,
    on_changed: Rc<dyn ScopeChangedCallback>,
    observed: ObservedIds,
}

impl ScopeEntry {
    fn new<T>(id: usize, scope: T, on_changed: Rc<dyn ScopeChangedCallback>) -> Self
    where
        T: Any + 'static,
    {
        Self {
            id,
            scope: Box::new(scope),
            on_changed,
            observed: ObservedIds::new(),
        }
    }

    fn callback_reusing<F: Fn(&dyn Any) + 'static>(
        &mut self,
        callback: F,
    ) -> Rc<dyn ScopeChangedCallback> {
        if let Some(stored) = Rc::get_mut(&mut self.on_changed)
            .and_then(|stored| (stored as &mut dyn Any).downcast_mut::<F>())
        {
            *stored = callback;
            Rc::clone(&self.on_changed)
        } else {
            Rc::new(callback)
        }
    }

    fn update<T>(&mut self, new_scope: &T, on_changed: Rc<dyn ScopeChangedCallback>)
    where
        T: Any + Clone + 'static,
    {
        self.update_scope(new_scope);
        self.on_changed = on_changed;
    }

    fn update_scope<T>(&mut self, new_scope: &T)
    where
        T: Any + Clone + 'static,
    {
        match self.scope.downcast_mut::<T>() {
            Some(stored) => stored.clone_from(new_scope),
            None => self.scope = Box::new(new_scope.clone()),
        }
    }

    fn matches_scope<T>(&self, scope: &T) -> bool
    where
        T: Any + Eq + 'static,
    {
        self.scope
            .downcast_ref::<T>()
            .is_some_and(|stored| stored == scope)
    }

    fn notify(&self) {
        (self.on_changed)(self.scope.as_ref());
    }
}

#[cfg(test)]
#[path = "tests/snapshot_state_observer_tests.rs"]
mod tests;
